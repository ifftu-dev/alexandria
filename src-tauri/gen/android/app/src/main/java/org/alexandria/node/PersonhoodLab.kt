package org.alexandria.node

import android.app.ActivityManager
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import org.json.JSONObject
import org.json.JSONArray
import java.math.BigInteger
import java.io.File
import java.net.HttpURLConnection
import java.util.concurrent.atomic.AtomicBoolean

internal object PersonhoodLab {
  private const val MEMORY_RESERVE = 1536L * 1024 * 1024
  private val lock = Any()
  @Volatile private var context: Context? = null
  private var active: Job? = null
  private var receiptId: String? = null
  private var receiptProof: JSONObject? = null
  private var receiptSignals: JSONArray? = null
  private var phase = "idle"
  private var error: String? = null
  private var result: JSONObject? = null
  private var elapsed = 0L
  private var received = 0L
  @Volatile private var foreground = false
  private val handler = Handler(Looper.getMainLooper())

  private class Job(val kind: String, val receipt: JSONObject? = null) {
    val started = SystemClock.elapsedRealtime()
    val cancelled = AtomicBoolean(false)
    @Volatile var process: Process? = null
    @Volatile var connection: HttpURLConnection? = null
  }

  fun initialize(app: Context) = synchronized(lock) {
    if (context == null) {
      context = app.applicationContext
      File(root(), "receipt-run").deleteRecursively()
    }
  }

  fun setForeground(value: Boolean) {
    foreground = value
    if (!value) cancel()
  }

  fun trimMemory(level: Int) {
    if (level == android.content.ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW ||
      level == android.content.ComponentCallbacks2.TRIM_MEMORY_RUNNING_CRITICAL) cancel()
  }

  private fun root(): File = File(checkNotNull(context).filesDir, "personhood-lab")
  private fun enabled(): Boolean = BuildConfig.DEBUG && BuildConfig.PERSONHOOD_LAB &&
    context?.let { File(it.applicationInfo.nativeLibraryDir, "libpersonhood_bench.so").isFile } == true

  fun dispatch(action: String): String {
    if (enabled() && action.startsWith("{")) return receiptDispatch(action)
    if (enabled()) {
      when (action) {
        "download", "prove" -> start(action)
        "cancel" -> cancel()
        "remove_key" -> synchronized(lock) {
          if (active == null) {
            if (root().exists() && !root().deleteRecursively()) {
              error = "Unable to remove test files; retry"
              phase = "error"
              return@synchronized
            }
            received = 0
            result = null
            error = null
            phase = "idle"
          }
        }
      }
    }
    return status()
  }

  private fun receiptDispatch(encoded: String): String {
    check(encoded.length <= 1024) { "Oversized receipt request" }
    val request = JSONObject(encoded)
    val id = request.getString("job_id")
    check(id.matches(Regex("[0-9a-f]{64}"))) { "Invalid receipt job" }
    when (request.getString("action")) {
      "receipt_prove" -> {
        for (field in listOf("signal_hash", "nullifier_seed")) {
          val value = request.getString(field)
          check(value.matches(Regex("0|[1-9][0-9]{0,77}")) &&
            BigInteger(value) < BigInteger("21888242871839275222246405745257275088548364400416034343698204186575808495617")) { "Invalid proof scalar" }
        }
        start("prove", request)
      }
      "receipt_discard" -> synchronized(lock) {
        if (receiptId == id) {
          cancel()
          receiptProof = null
          receiptSignals = null
          receiptId = null
        }
      }
      "receipt_status" -> Unit
      else -> error("Unknown receipt operation")
    }
    return synchronized(lock) {
      JSONObject().apply {
        put("job_id", receiptId ?: JSONObject.NULL)
        put("phase", phase)
        put("error", error ?: JSONObject.NULL)
        put("proof", if (receiptId == id && phase == "complete") receiptProof ?: JSONObject.NULL else JSONObject.NULL)
        put("public_signals", if (receiptId == id && phase == "complete") receiptSignals ?: JSONObject.NULL else JSONObject.NULL)
      }.toString()
    }
  }

  private fun status(): String = synchronized(lock) {
    val available = enabled()
    val key = if (available) File(root(), "circuit_final.zkey") else null
    val partial = if (available) File(root(), "circuit_final.zkey.partial") else null
    JSONObject().apply {
      put("enabled", available)
      put("phase", phase)
      put("key_status", if (key?.exists() == true) "stored" else if (partial?.exists() == true) "partial" else "missing")
      put("downloaded_bytes", if (key?.exists() == true) key.length() else partial?.length() ?: received)
      put("total_bytes", PersonhoodKeyTransfer.SIZE)
      put("elapsed_ms", active?.let { SystemClock.elapsedRealtime() - it.started } ?: elapsed)
      put("error", error ?: JSONObject.NULL)
      put("result", result ?: JSONObject.NULL)
    }.toString()
  }

  private fun lowMemory(): Boolean {
    val info = ActivityManager.MemoryInfo()
    (checkNotNull(context).getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager).getMemoryInfo(info)
    return info.lowMemory || info.availMem < MEMORY_RESERVE
  }

  private fun start(kind: String, receipt: JSONObject? = null) {
    val job = synchronized(lock) {
      if (active != null) return
      error = null
      result = null
      if (!foreground) {
        error = "Keep Alexandria visible while running the lab"
        phase = "error"
        return
      }
      if (kind == "prove" && lowMemory()) {
        error = "Not enough free memory; close other apps and retry"
        phase = "error"
        return
      }
      receiptId = receipt?.getString("job_id")
      receiptProof = null
      receiptSignals = null
      Job(kind, receipt).also { active = it; phase = if (kind == "download") "downloading" else "checking" }
    }
    MainActivity.personhoodKeepAwake(true)
    handler.post(object : Runnable {
      override fun run() {
        if (synchronized(lock) { active !== job }) return
        if (!foreground || (job.kind == "prove" && lowMemory())) cancel()
        handler.postDelayed(this, 500)
      }
    })
    Thread({ execute(job) }, "personhood-lab").start()
  }

  private fun checkCancelled(job: Job) {
    if (job.cancelled.get()) throw InterruptedException("cancelled")
  }

  private fun setPhase(job: Job, value: String) = synchronized(lock) {
    if (active === job && !job.cancelled.get()) phase = value
  }

  private fun execute(job: Job) {
    var outcome = "complete"
    var failure: String? = null
    var proofResult: JSONObject? = null
    try {
      val directory = root()
      check(directory.isDirectory || directory.mkdirs()) { "Cannot create lab storage" }
      if (job.kind == "download") {
        val existing = File(directory, "circuit_final.zkey")
        if (!PersonhoodKeyTransfer.verify(existing) { checkCancelled(job) }) {
          existing.delete()
          PersonhoodKeyTransfer.download(directory, { checkCancelled(job) },
            { connection -> job.connection = connection },
            { bytes -> synchronized(lock) { received = bytes } },
            { setPhase(job, "checking") })
        }
      } else {
        proofResult = prove(job, directory)
      }
    } catch (exception: Exception) {
      outcome = "error"
      failure = exception.message ?: "Lab operation failed; retry"
    } finally {
      job.process?.let { process ->
        if (process.isAlive) process.destroyForcibly()
        try { process.waitFor() } catch (_: InterruptedException) { Thread.currentThread().interrupt() }
      }
      if (job.kind == "prove" && (job.cancelled.get() || outcome != "complete" || job.receipt != null)) {
        val run = File(root(), if (job.receipt == null) "run" else "receipt-run")
        if (!run.deleteRecursively() && run.exists()) {
          outcome = "error"
          failure = "Unable to remove receipt test output"
        }
      }
      synchronized(lock) {
        if (active === job) {
          elapsed = SystemClock.elapsedRealtime() - job.started
          phase = if (job.cancelled.get()) "cancelled" else outcome
          error = if (job.cancelled.get()) null else failure
          result = if (job.cancelled.get()) null else proofResult
          if (job.cancelled.get() || outcome != "complete") {
            receiptProof = null
            receiptSignals = null
          }
          active = null
          MainActivity.personhoodKeepAwake(false)
        }
      }
    }
  }

  private fun prove(job: Job, directory: File): JSONObject {
    val key = File(directory, "circuit_final.zkey")
    check(key.exists()) { "Download the test key first" }
    if (!PersonhoodKeyTransfer.verify(key) { checkCancelled(job) }) {
      key.delete()
      error("Key checksum mismatch; download again")
    }
    checkCancelled(job)
    val output = File(directory, if (job.receipt == null) "run" else "receipt-run")
    output.deleteRecursively()
    check(output.mkdirs()) { "Cannot prepare synthetic test" }
    val app = checkNotNull(context)
    for (name in listOf("synthetic-input.json", "vkey.json")) {
      app.assets.open("personhood-lab/$name").use { input ->
        File(output, name).outputStream().use { input.copyTo(it) }
      }
    }
    job.receipt?.let { request ->
      val input = File(output, "synthetic-input.json")
      val values = JSONObject(input.readText())
      values.put("signalHash", request.getString("signal_hash"))
      values.put("nullifierSeed", request.getString("nullifier_seed"))
      input.writeText(values.toString())
    }
    val libraries = app.applicationInfo.nativeLibraryDir
    val builder = ProcessBuilder("$libraries/libpersonhood_bench.so",
      File(output, "synthetic-input.json").absolutePath, key.absolutePath,
      File(output, "vkey.json").absolutePath, File(output, "proof").absolutePath)
    builder.redirectErrorStream(true)
    builder.environment()["LD_LIBRARY_PATH"] = libraries
    builder.environment()["PERSONHOOD_PARENT_PID"] = android.os.Process.myPid().toString()
    val process = synchronized(job) {
      checkCancelled(job)
      builder.start().also { job.process = it }
    }
    process.inputStream.bufferedReader().useLines { lines ->
      lines.forEach { line ->
        if (line.startsWith("STAGE ")) {
          val stage = line.removePrefix("STAGE ")
          if (stage in listOf("witness", "proving", "verifying")) setPhase(job, stage)
        }
      }
    }
    val exit = process.waitFor()
    checkCancelled(job)
    check(exit == 0) { "Proof worker stopped (exit $exit); retry" }
    val native = JSONObject(File(output, "proof.results.json").readText())
    check(native.getBoolean("native_verified")) { "Synthetic proof did not verify" }
    if (job.receipt != null) {
      val proofFile = File(output, "proof.proof.json")
      val signalsFile = File(output, "proof.public.json")
      check(proofFile.length() <= 8192 && signalsFile.length() <= 2048) { "Oversized proof output" }
      val proof = JSONObject(proofFile.readText())
      val signals = JSONArray(signalsFile.readText())
      check(signals.length() == 9) { "Invalid proof output" }
      synchronized(lock) {
        checkCancelled(job)
        if (receiptId == job.receipt.getString("job_id")) {
          receiptProof = proof
          receiptSignals = signals
        }
      }
    }
    return JSONObject().apply {
      put("elapsed_ms", native.getDouble("elapsed_ms"))
      put("peak_rss_bytes", native.getLong("peak_rss_bytes"))
    }
  }

  private fun cancel() {
    val job = synchronized(lock) {
      val current = active ?: return
      if (!current.cancelled.compareAndSet(false, true)) return
      phase = "cancelling"
      current
    }
    synchronized(job) { job.process?.destroyForcibly() }
    Thread({ job.connection?.disconnect() }, "personhood-download-cancel").start()
  }
}
