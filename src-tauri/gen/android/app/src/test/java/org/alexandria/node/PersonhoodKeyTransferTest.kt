package org.alexandria.node

import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.io.RandomAccessFile

class PersonhoodKeyTransferTest {
  @Test fun fullResponseRestartsInsteadOfAppending() {
    assertEquals(0L, PersonhoodKeyTransfer.responseOffset(200, null, 10000))
  }

  @Test fun exactPinnedRangeResumes() {
    assertEquals(10000L, PersonhoodKeyTransfer.responseOffset(206, "bytes 10000-612082145/612082146", 10000))
  }

  @Test fun malformedOrUnrelatedRangesAreRejected() {
    for (range in listOf(null, "bytes 0-612082145/612082146", "bytes 10000-612082145/*",
      "bytes 10000-612082145/612082147", "bytes 10000-20000/612082146",
      "bytes 10000-9999/612082146", "bytes 10000-999999999999999999999999/612082146")) {
      assertFalse(PersonhoodKeyTransfer.validRange(range, 10000))
    }
  }

  @Test fun errorResponseCannotBecomeAKey() {
    for (code in listOf(301, 404, 416, 500)) {
      try {
        PersonhoodKeyTransfer.responseOffset(code, null, 12)
        fail("accepted HTTP $code")
      } catch (_: IllegalStateException) { }
    }
  }

  @Test fun incompleteKeyIsNeverVerified() {
    val file = File.createTempFile("personhood", ".zkey")
    try {
      file.writeBytes(byteArrayOf(1, 2, 3))
      assertFalse(PersonhoodKeyTransfer.verify(file) { })
    } finally { file.delete() }
  }

  @Test fun correctlySizedKeyWithWrongContentsIsRejected() {
    val file = File.createTempFile("personhood", ".zkey")
    try {
      RandomAccessFile(file, "rw").use { it.setLength(PersonhoodKeyTransfer.SIZE) }
      assertFalse(PersonhoodKeyTransfer.verify(file) { })
    } finally { file.delete() }
  }

  @Test fun checksumVerificationCanBeCancelled() {
    val file = File.createTempFile("personhood", ".zkey")
    try {
      RandomAccessFile(file, "rw").use { it.setLength(PersonhoodKeyTransfer.SIZE) }
      assertThrows(InterruptedException::class.java) {
        PersonhoodKeyTransfer.verify(file) { throw InterruptedException("cancelled") }
      }
    } finally { file.delete() }
  }
}
