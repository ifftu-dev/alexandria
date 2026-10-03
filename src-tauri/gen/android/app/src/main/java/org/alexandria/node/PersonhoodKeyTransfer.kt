package org.alexandria.node

import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

internal object PersonhoodKeyTransfer {
  const val SIZE = 612082146L
  const val SHA256 = "0d443ea85279b0370b53202f5e768ca9098cfcb5bb14f8ca8323d2266822c492"
  private const val URL_STRING = "https://anon-aadhaar-artifacts.s3.eu-central-1.amazonaws.com/v2.0.0/circuit_final.zkey"

  fun validRange(value: String?, offset: Long): Boolean {
    val match = Regex("bytes ([0-9]+)-([0-9]+)/([0-9]+)").matchEntire(value ?: "") ?: return false
    val start = match.groupValues[1].toLongOrNull() ?: return false
    val end = match.groupValues[2].toLongOrNull() ?: return false
    val total = match.groupValues[3].toLongOrNull() ?: return false
    return start == offset && end >= start && end == SIZE - 1 && total == SIZE
  }

  fun responseOffset(code: Int, range: String?, offset: Long): Long {
    if (code == 200) return 0
    check(code == 206 && validRange(range, offset)) { "Invalid resume response (HTTP $code); retry the download" }
    return offset
  }

  fun verify(file: File, checkCancelled: () -> Unit): Boolean {
    if (file.length() != SIZE) return false
    val hash = MessageDigest.getInstance("SHA-256")
    FileInputStream(file).use { input ->
      val buffer = ByteArray(1024 * 1024)
      while (true) {
        checkCancelled()
        val count = input.read(buffer)
        if (count < 0) break
        hash.update(buffer, 0, count)
      }
    }
    return hash.digest().joinToString("") { "%02x".format(it.toInt() and 255) } == SHA256
  }

  fun download(
    root: File,
    checkCancelled: () -> Unit,
    connectionChanged: (HttpURLConnection?) -> Unit,
    progress: (Long) -> Unit,
    checking: () -> Unit,
  ) {
    val partial = File(root, "circuit_final.zkey.partial")
    var offset = partial.length()
    if (offset > SIZE) {
      check(partial.delete()) { "Cannot discard oversized partial key" }
      offset = 0
    }
    check(root.usableSpace >= SIZE - offset + 64L * 1024 * 1024) { "Not enough storage for the 612 MB test key" }
    if (offset < SIZE) {
      checkCancelled()
      val connection = URL(URL_STRING).openConnection() as HttpURLConnection
      connection.instanceFollowRedirects = false
      connection.connectTimeout = 15000
      connection.readTimeout = 10000
      connection.setRequestProperty("Accept-Encoding", "identity")
      if (offset > 0) connection.setRequestProperty("Range", "bytes=$offset-")
      connectionChanged(connection)
      try {
        checkCancelled()
        val code = connection.responseCode
        val append = code == 206
        offset = responseOffset(code, connection.getHeaderField("Content-Range"), offset)
        check(connection.contentEncoding == null || connection.contentEncoding == "identity") { "Unexpected key encoding" }
        val length = connection.contentLengthLong
        check(length == -1L || length == SIZE - offset) { "Unexpected key download length" }
        FileOutputStream(partial, append).use { output ->
          connection.inputStream.use { input ->
            val buffer = ByteArray(256 * 1024)
            var received = offset
            progress(received)
            while (true) {
              checkCancelled()
              val count = input.read(buffer)
              if (count < 0) break
              check(received + count <= SIZE) { "Key exceeds the expected size" }
              output.write(buffer, 0, count)
              received += count
              progress(received)
            }
            check(received == SIZE) { "Download interrupted; retry to resume" }
          }
          output.fd.sync()
        }
      } finally {
        connectionChanged(null)
        connection.disconnect()
      }
    }
    checkCancelled()
    checking()
    if (!verify(partial, checkCancelled)) {
      partial.delete()
      error("Key checksum mismatch; download again")
    }
    checkCancelled()
    check(partial.renameTo(File(root, "circuit_final.zkey"))) { "Cannot finalize verified key" }
  }
}
