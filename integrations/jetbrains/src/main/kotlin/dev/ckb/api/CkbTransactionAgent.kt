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
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * UI-neutral transaction API exported to trusted JetBrains agents.
 * Source and workspace paths stay local; Cloud receives hashes, identifiers
 * and bounded CKB evidence only. No operation merges or pushes.
 */
object CkbTransactionAgent {
    private val gson = Gson()
    private val json = "application/json; charset=utf-8".toMediaType()
    private val credential = CredentialAttributes("CKB Cloud Architecture Transaction API Key")

    fun setCloudApiKey(apiKey: String) {
        require(apiKey.isBlank() || apiKey.startsWith("ckb_live_")) { "CKB Cloud API keys must start with ckb_live_" }
        val normalized = apiKey.trim()
        PasswordSafe.instance.setPassword(credential, if (normalized.isBlank()) null else normalized)
    }

    private fun apiKey(): String = PasswordSafe.instance.getPassword(credential)
        ?: throw IllegalStateException("A ckb_live_ Cloud API key is required for architecture transactions")

    private fun client() = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(180, TimeUnit.SECONDS)
        .build()

    private fun cloud(path: String, body: JsonObject): JsonObject {
        val base = CkbSettings.instance.cloudApiUrl.trimEnd('/')
        val request = Request.Builder()
            .url("$base/api/v1/mcp$path")
            .header("Authorization", "Bearer ${apiKey()}")
            .header("User-Agent", "CKB-JetBrains-Transaction-Agent/1.0")
            .post(gson.toJson(body).toRequestBody(json))
            .build()
        return client().newCall(request).execute().use { response ->
            val raw = response.body?.string().orEmpty()
            val parsed = runCatching { gson.fromJson(raw, JsonObject::class.java) }.getOrNull()
            if (!response.isSuccessful) {
                throw IllegalStateException(parsed?.get("message")?.asString ?: raw.ifBlank { "CKB Cloud returned HTTP ${response.code}" })
            }
            parsed ?: JsonObject()
        }
    }

    private fun state(path: String): JsonObject = gson.fromJson(File(path).readText(), JsonObject::class.java)

    fun prepare(
        projectPath: String,
        projectId: String,
        instruction: String,
        target: JsonObject,
        patchFile: String,
        validationFile: String,
        stateFile: String,
        baseline: String = "HEAD"
    ): JsonObject {
        val capsuleRequest = JsonObject().apply {
            addProperty("instruction", instruction)
            addProperty("project_id", projectId)
            add("context", JsonObject().apply { add("selectedNode", target) })
        }
        val capsule = cloud("/architecture/prepare-change", capsuleRequest)
        val local = CkbIntelligenceClient.preparePatch(projectPath, patchFile, validationFile, stateFile, baseline)
        val transaction = local.getAsJsonObject("transaction")
        val validation = JsonObject().apply {
            addProperty("snapshotId", capsule.get("snapshotId").asString)
            addProperty("baselineCommit", transaction.get("baseline_commit").asString)
            addProperty("patchObjectId", transaction.get("patch_object_id").asString)
            addProperty("stagedTreeId", transaction.get("staged_tree_id").asString)
            addProperty("branchName", transaction.get("branch_name").asString)
            addProperty("validationSucceeded", transaction.get("state").asString == "validated")
            add("validation", transaction.get("validations"))
        }
        val recorded = cloud("/architecture/transactions/${capsule.get("capsuleId").asString}/validation", validation)
        return JsonObject().apply {
            add("capsule", capsule); add("local", local); add("recorded", recorded)
            addProperty("mutationApplied", false); addProperty("activeCheckoutModified", false); addProperty("synthetic", false)
        }
    }

    fun confirmAndCommit(
        projectPath: String,
        capsuleId: String,
        snapshotId: String,
        stagedTreeId: String,
        stateFile: String,
        message: String
    ): JsonObject {
        var transaction = state(stateFile)
        require(transaction.get("staged_tree_id").asString == stagedTreeId) { "Confirmation does not match the local staged tree" }
        cloud("/architecture/transactions/$capsuleId/confirm", JsonObject().apply {
            addProperty("snapshotId", snapshotId); addProperty("stagedTreeId", stagedTreeId)
        })
        val local = if (transaction.get("state").asString == "committed") JsonObject().apply {
            addProperty("committedSha", transaction.get("committed_sha").asString); addProperty("resumed", true)
        } else CkbIntelligenceClient.commitPatch(projectPath, stateFile, stagedTreeId, message)
        transaction = state(stateFile)
        val recorded = cloud("/architecture/transactions/$capsuleId/committed", JsonObject().apply {
            addProperty("stagedTreeId", stagedTreeId); addProperty("committedSha", transaction.get("committed_sha").asString)
        })
        return JsonObject().apply {
            add("local", local); add("recorded", recorded); addProperty("merged", false)
            addProperty("pushed", false); addProperty("activeCheckoutModified", false); addProperty("synthetic", false)
        }
    }

    fun rescan(projectPath: String, capsuleId: String, stateFile: String): JsonObject {
        val local = CkbIntelligenceClient.rescanPatch(projectPath, stateFile)
        val observed = local.get("rollbackCommittedSha")?.takeUnless { it.isJsonNull }?.asString
            ?: local.get("committedSha").asString
        val evidence = JsonObject().apply {
            add("scan", local.get("scan")); add("activity", local.get("activity")); add("dna", local.get("dna")); add("memory", local.get("memory"))
            addProperty("evidencePolicy", local.get("evidencePolicy").asString); addProperty("activeCheckoutModified", false); addProperty("synthetic", false)
        }
        val recorded = cloud("/architecture/transactions/$capsuleId/rescan", JsonObject().apply {
            addProperty("observedCommitSha", observed); addProperty("snapshotId", local.getAsJsonObject("scan").get("snapshot_id").asString)
            add("validations", local.get("validations")); add("evidence", evidence)
        })
        return JsonObject().apply { add("local", local); add("recorded", recorded); addProperty("synthetic", false) }
    }

    fun rollback(projectPath: String, capsuleId: String, stateFile: String, committedSha: String): JsonObject {
        var transaction = state(stateFile)
        val local = if (transaction.get("state").asString == "rolled-back") JsonObject().apply { addProperty("resumed", true) }
        else CkbIntelligenceClient.rollbackPatch(projectPath, stateFile, committedSha)
        transaction = state(stateFile)
        val recorded = cloud("/architecture/transactions/$capsuleId/rollback", JsonObject().apply {
            addProperty("committedSha", committedSha)
            addProperty("rollbackStagedTreeId", transaction.get("rollback_staged_tree_id").asString)
            addProperty("rollbackCommitSha", transaction.get("rollback_committed_sha").asString)
            add("validations", transaction.get("rollback_validations"))
        })
        return JsonObject().apply {
            add("local", local); add("recorded", recorded); addProperty("merged", false)
            addProperty("pushed", false); addProperty("activeCheckoutModified", false); addProperty("synthetic", false)
        }
    }
}
