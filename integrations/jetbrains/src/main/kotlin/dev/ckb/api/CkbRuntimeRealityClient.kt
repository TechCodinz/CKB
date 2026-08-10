package dev.ckb.api

import com.google.gson.Gson
import com.google.gson.JsonObject
import dev.ckb.settings.CkbSettings
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit

/** Read-only runtime client for a configured CKB Reality server. */
object CkbRuntimeRealityClient {
    private val gson = Gson()
    private val client = OkHttpClient.Builder()
        .connectTimeout(4, TimeUnit.SECONDS)
        .readTimeout(8, TimeUnit.SECONDS)
        .build()

    private fun get(path: String): JsonObject {
        val base = CkbSettings.instance.serverUrl.trimEnd('/')
        val request = Request.Builder()
            .url("$base$path")
            .get()
            .header("Accept", "application/json")
            .build()
        return client.newCall(request).execute().use { response ->
            if (!response.isSuccessful) throw RuntimeException("CKB Runtime Reality returned HTTP ${response.code}")
            val raw = response.body?.string().orEmpty()
            if (raw.isBlank()) JsonObject() else gson.fromJson(raw, JsonObject::class.java)
        }
    }

    fun traces(): JsonObject = get("/api/v1/intelligence/traces")
    fun runtime(): JsonObject = get("/api/v1/intelligence/runtime")

    fun snapshot(): Pair<JsonObject, JsonObject> = traces() to runtime()
}
