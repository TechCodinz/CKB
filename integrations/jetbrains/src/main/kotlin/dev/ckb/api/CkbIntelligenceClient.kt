package dev.ckb.api

import com.google.gson.Gson
import com.google.gson.JsonObject
import dev.ckb.settings.CkbSettings
import java.io.File
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

/**
 * Local, process-isolated bridge to `ckb-intelligence`.
 *
 * No workspace path is sent to a remote machine. The binary scans the same
 * local project the IDE has open, then emits evidence-backed JSON for deep
 * activity, Code DNA and bounded architecture memory.
 */
object CkbIntelligenceClient {
    private val gson = Gson()

    private fun run(projectPath: String, args: List<String>, timeoutSeconds: Long = 180): JsonObject {
        val executable = CkbSettings.instance.intelligenceBinary.ifBlank { "ckb-intelligence" }
        val command = mutableListOf(executable)
        command.addAll(args)

        val process = ProcessBuilder(command)
            .directory(File(projectPath))
            .redirectErrorStream(false)
            .start()

        // Drain both pipes while the process runs. Deep memory bundles can be
        // much larger than the OS pipe buffer; waiting before reading can
        // otherwise deadlock the extension on large repositories.
        val stdout = StringBuilder()
        val stderr = StringBuilder()
        val outThread = thread(name = "ckb-intelligence-stdout", isDaemon = true) {
            process.inputStream.bufferedReader().use { reader ->
                val buffer = CharArray(8192)
                while (true) {
                    val count = reader.read(buffer)
                    if (count < 0) break
                    stdout.append(buffer, 0, count)
                }
            }
        }
        val errThread = thread(name = "ckb-intelligence-stderr", isDaemon = true) {
            process.errorStream.bufferedReader().use { reader ->
                val buffer = CharArray(4096)
                while (true) {
                    val count = reader.read(buffer)
                    if (count < 0) break
                    stderr.append(buffer, 0, count)
                }
            }
        }

        val completed = process.waitFor(timeoutSeconds, TimeUnit.SECONDS)
        if (!completed) {
            process.destroyForcibly()
            outThread.join(1000)
            errThread.join(1000)
            throw RuntimeException("CKB deep architecture analysis timed out after ${timeoutSeconds}s")
        }
        outThread.join(2000)
        errThread.join(2000)

        val stdoutText = stdout.toString().trim()
        val stderrText = stderr.toString().trim()
        if (stdoutText.isNotBlank()) {
            try {
                return gson.fromJson(stdoutText, JsonObject::class.java)
            } catch (_: Exception) {
                // Preserve the actual process failure below.
            }
        }
        if (process.exitValue() != 0) {
            throw RuntimeException(stderrText.ifBlank { "CKB intelligence process exited with ${process.exitValue()}" })
        }
        throw RuntimeException("CKB intelligence process returned no JSON output")
    }

    fun bundle(projectPath: String, query: String = "architecture hotspots dependencies runtime change risk"): JsonObject =
        run(projectPath, listOf("bundle", projectPath, "--query", query, "--depth", "3", "--limit", "36"))

    fun activity(projectPath: String): JsonObject =
        run(projectPath, listOf("activity", projectPath))

    fun memory(projectPath: String, query: String, depth: Int = 3, limit: Int = 36): JsonObject =
        run(projectPath, listOf("memory", projectPath, query, "--depth", depth.toString(), "--limit", limit.toString()))

    fun dna(projectPath: String): JsonObject =
        run(projectPath, listOf("dna", projectPath))
}
