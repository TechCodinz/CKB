package dev.ckb.api

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.intellij.credentialStore.CredentialAttributes
import com.intellij.ide.passwordSafe.PasswordSafe
import dev.ckb.settings.CkbSettings
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.net.URLEncoder
import java.nio.charset.StandardCharsets
import java.util.concurrent.TimeUnit

/**
 * Model-neutral Cloud intelligence client shared by JetBrains actions/agents.
 * Provider/model identity is metadata only; all returned evidence is compiled
 * by CKB's V13 truth contract rather than by IDE-side heuristics.
 */
object CkbModelIntelligenceV13 {
    private val gson = Gson()
    private val json = "application/json; charset=utf-8".toMediaType()
    private val credential = CredentialAttributes("CKB Cloud Architecture Transaction API Key")

    private fun apiKey(): String = PasswordSafe.instance.getPassword(credential)
        ?: throw IllegalStateException("A ckb_live_ Cloud API key is required. Configure the same key used by CKB Guarded Change.")

    private fun client() = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(180, TimeUnit.SECONDS)
        .build()

    private fun baseUrl(): String = CkbSettings.instance.cloudApiUrl.trimEnd('/')

    private fun execute(request: Request): JsonObject = client().newCall(request).execute().use { response ->
        val raw = response.body?.string().orEmpty()
        val parsed = runCatching { gson.fromJson(raw, JsonObject::class.java) }.getOrNull()
        if (!response.isSuccessful) {
            throw IllegalStateException(parsed?.get("message")?.asString ?: raw.ifBlank { "CKB Cloud returned HTTP ${response.code}" })
        }
        parsed ?: JsonObject()
    }

    private fun post(path: String, body: JsonObject): JsonObject {
        val request = Request.Builder()
            .url("${baseUrl()}/api/v1/mcp$path")
            .header("Authorization", "Bearer ${apiKey()}")
            .header("User-Agent", "CKB-JetBrains-Intelligence-Fabric/13")
            .post(gson.toJson(body).toRequestBody(json))
            .build()
        return execute(request)
    }

    private fun get(path: String, query: Map<String, String> = emptyMap()): JsonObject {
        val suffix = if (query.isEmpty()) "" else query.entries.joinToString("&", prefix = "?") {
            "${URLEncoder.encode(it.key, StandardCharsets.UTF_8)}=${URLEncoder.encode(it.value, StandardCharsets.UTF_8)}"
        }
        val request = Request.Builder()
            .url("${baseUrl()}/api/v1/mcp$path$suffix")
            .header("Authorization", "Bearer ${apiKey()}")
            .header("User-Agent", "CKB-JetBrains-Intelligence-Fabric/13")
            .get()
            .build()
        return execute(request)
    }

    fun compileContext(
        projectId: String,
        query: String,
        task: String,
        path: String? = null,
        line: Int? = null,
        symbol: String? = null,
        modelProfile: JsonObject? = null,
    ): JsonObject {
        val request = JsonObject().apply {
            addProperty("project_id", projectId.ifBlank { "current" })
            addProperty("query", query)
            addProperty("task", task)
            addProperty("depth", if (task in setOf("change", "debug", "security")) 3 else 2)
            addProperty("limit", 120)
            add("budget", JsonObject().apply {
                addProperty("maxChars", 48_000)
                addProperty("maxNodes", 80)
                addProperty("maxEdges", 160)
            })
            if (modelProfile != null) add("modelProfile", modelProfile)
            if (!path.isNullOrBlank()) {
                add("cursorContext", JsonObject().apply {
                    addProperty("path", path)
                    if (line != null) addProperty("line", line)
                    if (!symbol.isNullOrBlank()) addProperty("symbol", symbol)
                })
            }
        }
        return post("/architecture/context/compile", request)
    }

    fun observedModelRegistry(projectId: String, task: String): JsonObject = get(
        "/architecture/models/observed-registry",
        mapOf("project_id" to projectId.ifBlank { "current" }, "task" to task),
    )

    fun frontierCatalog(): JsonObject = get("/architecture/models/catalog")

    fun adaptFrontierRequest(provider: String, model: String, requestJson: JsonObject): JsonObject = post(
        "/architecture/models/request-adapt",
        JsonObject().apply {
            addProperty("provider", provider)
            addProperty("model", model)
            add("request", requestJson)
        },
    )

    fun constitution(): JsonObject = get("/architecture/constitution")

    fun fabricManifest(): JsonObject = get("/architecture/fabric/manifest")
}
