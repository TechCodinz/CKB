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
    // The Rust server serializes `from`/`to` as plain strings: they are
    // `NodeId(pub String)` newtypes in ckb-core (transparent serde), NOT
    // objects. Typing these as Map made Gson throw JsonSyntaxException
    // on any report containing a real violation.
    val from: String = "",
    val to: String = "",
    val boundary: String = "",
    val message: String = "",
    val severity: String = "",
    val suggested_fix: String? = null
)

// Mirrors ckb-core's `ImpactedNode` (core/src/types/mod.rs): `node` is a
// NodeId (plain string), `impact_kind` an enum name (e.g. "CompileBreak"),
// `path` a filesystem path string, `line` a 1-based line number.
data class ImpactedNode(
    val node: String = "",
    val impact_kind: String = "",
    val confidence: Double = 0.0,
    val path: String = "",
    val line: Int = 0
)

// Mirrors ckb-core's `ImpactAnalysis`: the fields are `direct_impacts` /
// `indirect_impacts` of ImpactedNode objects. (The previous
// directly_affected/transitively_affected List<String> fields don't
// exist in the server JSON, so Gson silently left them empty and
// "Analyze Impact" always showed no affected nodes.)
data class ImpactAnalysis(
    val direct_impacts: List<ImpactedNode> = emptyList(),
    val indirect_impacts: List<ImpactedNode> = emptyList(),
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

    // The server enforces CKB_API_KEY on every /api/v1/* route when one
    // is configured; mirror the VS Code extension (which sends the
    // configured key as X-API-Key on every API call). Without this,
    // every call against an auth-enabled server 401s.
    private fun Request.Builder.withApiKey(): Request.Builder {
        val key = dev.ckb.settings.CkbSettings.instance.apiKey.trim()
        if (key.isNotEmpty()) {
            header("X-API-Key", key)
            header("Authorization", "Bearer $key")
        }
        return this
    }

    fun health(): Boolean {
        return try {
            val request = Request.Builder().url("${baseUrl()}/health").get().withApiKey().build()
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
            .withApiKey()
            .build()
        val response = getClient(120).newCall(request).execute()
        response.use {
            if (!it.isSuccessful) throw RuntimeException("Scan failed: ${it.code}")
            // After scan, fetch report
            return getReport()
        }
    }

    fun getReport(): ScanReport {
        val request = Request.Builder().url("${baseUrl()}/api/v1/report").get().withApiKey().build()
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
            .withApiKey()
            .build()
        val response = getClient(30).newCall(request).execute()
        return response.use {
            if (!it.isSuccessful) throw RuntimeException("Impact analysis failed: ${it.code}")
            gson.fromJson(it.body?.string(), ImpactAnalysis::class.java)
        }
    }
}
