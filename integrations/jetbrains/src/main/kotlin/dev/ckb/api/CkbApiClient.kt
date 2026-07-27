package dev.ckb.api

import com.google.gson.Gson
import com.google.gson.JsonElement
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

data class ScanReport(
    val files_processed: Int = 0,
    val nodes: Int = 0,
    val edges: Int = 0,
    val patterns: List<JsonElement> = emptyList(),
    val drift: List<DriftViolation> = emptyList(),
    val snapshot_id: String = ""
)

data class DriftViolation(
    val id: String = "",
    val kind: String = "",
    val from: Map<String, String> = emptyMap(),
    val to: Map<String, String> = emptyMap(),
    val boundary: String = "",
    val message: String = "",
    val severity: String = "",
    val suggested_fix: String? = null
)

data class ImpactAnalysis(
    val directly_affected: List<String> = emptyList(),
    val transitively_affected: List<String> = emptyList(),
    val risk_score: Double = 0.0,
    val estimated_effort: String = "unknown"
)

object CkbApiClient {
    private val gson = Gson()
    private val json = "application/json; charset=utf-8".toMediaType()

    private fun getClient(timeoutSeconds: Long = 60): OkHttpClient =
        OkHttpClient.Builder()
            .connectTimeout(5, TimeUnit.SECONDS)
            .readTimeout(timeoutSeconds, TimeUnit.SECONDS)
            .build()

    private fun baseUrl(): String = dev.ckb.settings.CkbSettings.instance.serverUrl

    fun health(): Boolean {
        return try {
            val request = Request.Builder().url("${baseUrl()}/health").get().build()
            getClient(5).newCall(request).execute().use { it.isSuccessful }
        } catch (e: Exception) {
            false
        }
    }

    fun scan(projectPath: String): ScanReport {
        val body = gson.toJson(mapOf("path" to projectPath)).toRequestBody(json)
        val request = Request.Builder()
            .url("${baseUrl()}/api/v1/scan")
            .post(body)
            .build()
        val response = getClient(120).newCall(request).execute()
        response.use {
            if (!it.isSuccessful) throw RuntimeException("Scan failed: ${it.code}")
            // After scan, fetch report
            return getReport()
        }
    }

    fun getReport(): ScanReport {
        val request = Request.Builder().url("${baseUrl()}/api/v1/report").get().build()
        val response = getClient(30).newCall(request).execute()
        return response.use {
            if (it.code == 404) return ScanReport()
            if (!it.isSuccessful) throw RuntimeException("Failed to get report: ${it.code}")
            gson.fromJson(it.body?.string(), ScanReport::class.java)
        }
    }

    fun analyzeImpact(projectPath: String, filePath: String, line: Int, changeType: String = "modify"): ImpactAnalysis {
        val body = gson.toJson(mapOf(
            "path" to projectPath,
            "file" to filePath,
            "line" to line,
            "change_type" to changeType
        )).toRequestBody(json)
        val request = Request.Builder()
            .url("${baseUrl()}/api/v1/impact")
            .post(body)
            .build()
        val response = getClient(30).newCall(request).execute()
        return response.use {
            if (!it.isSuccessful) throw RuntimeException("Impact analysis failed: ${it.code}")
            gson.fromJson(it.body?.string(), ImpactAnalysis::class.java)
        }
    }
}
