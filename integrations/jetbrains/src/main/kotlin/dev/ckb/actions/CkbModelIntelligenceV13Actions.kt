package dev.ckb.actions

import com.google.gson.GsonBuilder
import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.Task
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiNamedElement
import dev.ckb.api.CkbModelIntelligenceV13
import java.io.File

private val V13_TASKS = arrayOf("understand", "explain", "change", "debug", "review", "migrate", "optimize", "security")
private val prettyGson = GsonBuilder().setPrettyPrinting().create()

private data class V13Cursor(val path: String, val line: Int, val column: Int, val symbol: String?)
private data class ModelPick(val cancelled: Boolean, val profile: JsonObject?)

private fun v13Cursor(e: AnActionEvent): V13Cursor? {
    val project = e.project ?: return null
    val rootText = project.basePath ?: return null
    val editor = FileEditorManager.getInstance(project).selectedTextEditor ?: return null
    val virtualFile = FileDocumentManager.getInstance().getFile(editor.document) ?: return null
    val root = runCatching { File(rootText).canonicalFile.toPath() }.getOrNull() ?: return null
    val source = runCatching { File(virtualFile.path).canonicalFile.toPath() }.getOrNull() ?: return null
    if (!source.startsWith(root)) return null
    val relative = root.relativize(source).toString().replace('\\', '/')
    if (relative.isBlank() || relative.startsWith("../")) return null

    val offset = editor.caretModel.offset.coerceIn(0, maxOf(0, editor.document.textLength - 1))
    val psiFile = PsiDocumentManager.getInstance(project).getPsiFile(editor.document)
    var element = if (psiFile != null && editor.document.textLength > 0) psiFile.findElementAt(offset) else null
    var symbol: String? = null
    while (element != null) {
        if (element is PsiNamedElement && !element.name.isNullOrBlank()) {
            symbol = element.name
            break
        }
        element = element.parent
    }
    return V13Cursor(relative, editor.caretModel.logicalPosition.line + 1, editor.caretModel.logicalPosition.column + 1, symbol)
}

private fun chooseTask(e: AnActionEvent): String? {
    val project = e.project ?: return null
    val index = Messages.showChooseDialog(
        project,
        "Choose how CKB should compile architecture memory for the model/agent.",
        "CKB V13 Architecture Task",
        null,
        V13_TASKS.map { it.uppercase() }.toTypedArray(),
        V13_TASKS[0].uppercase(),
    )
    return if (index in V13_TASKS.indices) V13_TASKS[index] else null
}

private fun openJsonResult(e: AnActionEvent, title: String, value: Any) {
    val project = e.project ?: return
    val temp = kotlin.io.path.createTempFile("ckb-v13-", ".json").toFile()
    temp.writeText(prettyGson.toJson(value))
    temp.deleteOnExit()
    val file = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(temp)
    if (file != null) {
        FileEditorManager.getInstance(project).openFile(file, true)
    } else {
        Messages.showInfoMessage(project, prettyGson.toJson(value).take(12_000), title)
    }
}

private fun background(e: AnActionEvent, title: String, operation: () -> Any) {
    val project = e.project ?: return
    ProgressManager.getInstance().run(object : Task.Backgroundable(project, title, true) {
        override fun run(indicator: ProgressIndicator) {
            indicator.isIndeterminate = true
            try {
                val result = operation()
                ApplicationManager.getApplication().invokeLater { openJsonResult(e, title, result) }
            } catch (error: Exception) {
                ApplicationManager.getApplication().invokeLater {
                    Messages.showErrorDialog(project, error.message ?: error.toString(), title)
                }
            }
        }
    })
}

private fun catalogEntries(catalog: JsonObject): List<JsonObject> =
    catalog.getAsJsonArray("entries")?.mapNotNull { element ->
        if (element.isJsonObject) element.asJsonObject else null
    } ?: emptyList()

private fun profileLabel(profile: JsonObject): String {
    val provider = profile.get("provider")?.asString ?: "unknown"
    val model = profile.get("model")?.asString ?: "unknown"
    val availability = profile.get("availability")?.asString ?: "unknown"
    val freshness = profile.get("freshness")?.asString ?: "unknown"
    val selectable = profile.get("selectable")?.asBoolean != false
    return "$provider/$model  [$availability • $freshness • ${if (selectable) "selectable" else "migration-only"}]"
}

private fun chooseModel(project: com.intellij.openapi.project.Project, entries: List<JsonObject>, allowNeutral: Boolean): ModelPick {
    // For architecture context hints, use only fresh/selectable verified
    // profiles. Migration/request inspection keeps every lifecycle state visible.
    val candidates = if (allowNeutral) entries.filter { it.get("selectable")?.asBoolean != false } else entries
    val labels = mutableListOf<String>()
    if (allowNeutral) labels += "CKB MODEL-NEUTRAL  [no provider assumptions]"
    labels += candidates.map(::profileLabel)
    if (labels.isEmpty()) {
        Messages.showWarningDialog(project, "CKB verified frontier model catalog is empty.", "CKB V13")
        return ModelPick(true, null)
    }
    val index = Messages.showChooseDialog(
        project,
        "Capability metadata changes request/context hints only. It never changes CKB evidence truth or acts as an unobserved quality score.",
        "CKB V13 Verified Frontier Model",
        null,
        labels.toTypedArray(),
        labels.first(),
    )
    if (index !in labels.indices) return ModelPick(true, null)
    if (allowNeutral && index == 0) return ModelPick(false, null)
    val modelIndex = index - if (allowNeutral) 1 else 0
    return ModelPick(false, candidates.getOrNull(modelIndex))
}

private fun isSupported(value: String?): Boolean = value in setOf("supported", "preview", "beta")

private fun contextProfile(profile: JsonObject): JsonObject {
    val tools = profile.getAsJsonObject("tools") ?: JsonObject()
    val modalities = profile.getAsJsonArray("inputModalities")?.mapNotNull { it.takeIf { value -> value.isJsonPrimitive }?.asString } ?: emptyList()
    return JsonObject().apply {
        addProperty("provider", profile.get("provider")?.asString)
        addProperty("model", profile.get("model")?.asString)
        profile.get("contextWindowTokens")?.takeIf { !it.isJsonNull }?.let { add("contextWindowTokens", it) }
        addProperty("supportsStructuredOutput", isSupported(tools.get("structuredOutput")?.asString))
        addProperty("supportsToolUse", isSupported(tools.get("functionCalling")?.asString))
        addProperty("supportsParallelTools", isSupported(tools.get("parallelFunctionCalling")?.asString))
        addProperty("supportsImages", "image" in modalities)
        addProperty("supportsCodeExecution", isSupported(tools.get("codeExecution")?.asString))
    }
}

private fun withCatalog(e: AnActionEvent, title: String, onSuccess: (List<JsonObject>) -> Unit) {
    val project = e.project ?: return
    ProgressManager.getInstance().run(object : Task.Backgroundable(project, title, true) {
        override fun run(indicator: ProgressIndicator) {
            indicator.isIndeterminate = true
            try {
                val entries = catalogEntries(CkbModelIntelligenceV13.frontierCatalog())
                ApplicationManager.getApplication().invokeLater { onSuccess(entries) }
            } catch (error: Exception) {
                ApplicationManager.getApplication().invokeLater {
                    Messages.showErrorDialog(project, error.message ?: error.toString(), title)
                }
            }
        }
    })
}

class CompileArchitectureContextV13Action : AnAction("Compile Model Architecture Context at Cursor") {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val cursor = v13Cursor(e) ?: run {
            Messages.showWarningDialog(project, "Open a source file inside the current project first.", "CKB V13")
            return
        }
        val task = chooseTask(e) ?: return
        val defaultQuery = "$task ${cursor.symbol ?: File(cursor.path).name} at ${cursor.path}:${cursor.line}"
        val query = Messages.showInputDialog(
            project,
            "Describe the task. CKB will compile bounded evidence, not dump the repository.",
            "CKB V13 Context Compiler",
            null,
            defaultQuery,
            null,
        )?.trim().orEmpty()
        if (query.isBlank()) return

        withCatalog(e, "CKB V13 • Load Verified Frontier Profiles") { entries ->
            val picked = chooseModel(project, entries, allowNeutral = true)
            if (picked.cancelled) return@withCatalog
            background(e, "CKB V13 • Compile Architecture Context") {
                CkbModelIntelligenceV13.compileContext(
                    "current",
                    query,
                    task,
                    cursor.path,
                    cursor.line,
                    cursor.symbol,
                    picked.profile?.let(::contextProfile),
                )
            }
        }
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.project != null && v13Cursor(e) != null
    }
}

class ShowObservedModelRegistryV13Action : AnAction("Show Observed Model Registry") {
    override fun actionPerformed(e: AnActionEvent) {
        val task = chooseTask(e) ?: return
        background(e, "CKB V13 • Observed Model Registry") {
            CkbModelIntelligenceV13.observedModelRegistry("current", task)
        }
    }

    override fun update(e: AnActionEvent) { e.presentation.isEnabled = e.project != null }
}

class ShowVerifiedFrontierModelCatalogV13Action : AnAction("Show Verified Frontier Model Catalog") {
    override fun actionPerformed(e: AnActionEvent) {
        background(e, "CKB V13 • Verified Frontier Model Catalog") { CkbModelIntelligenceV13.frontierCatalog() }
    }

    override fun update(e: AnActionEvent) { e.presentation.isEnabled = e.project != null }
}

class CheckFrontierModelRequestV13Action : AnAction("Check JSON Request Against Frontier Model") {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val editor = FileEditorManager.getInstance(project).selectedTextEditor ?: run {
            Messages.showWarningDialog(project, "Open a JSON request document first.", "CKB V13")
            return
        }
        val requestJson = runCatching {
            JsonParser.parseString(editor.document.text).asJsonObject
        }.getOrElse {
            Messages.showWarningDialog(project, "The active editor must contain a JSON object request.", "CKB V13")
            return
        }
        withCatalog(e, "CKB V13 • Load Verified Frontier Profiles") { entries ->
            val picked = chooseModel(project, entries, allowNeutral = false)
            val profile = picked.profile
            if (picked.cancelled || profile == null) return@withCatalog
            val provider = profile.get("provider")?.asString ?: return@withCatalog
            val model = profile.get("model")?.asString ?: return@withCatalog
            background(e, "CKB V13 • Frontier Request Compatibility") {
                CkbModelIntelligenceV13.adaptFrontierRequest(provider, model, requestJson)
            }
        }
    }

    override fun update(e: AnActionEvent) { e.presentation.isEnabled = e.project != null }
}

class ShowArchitectureConstitutionV13Action : AnAction("Show Architecture Constitution") {
    override fun actionPerformed(e: AnActionEvent) {
        background(e, "CKB V13 • Architecture Constitution") { CkbModelIntelligenceV13.constitution() }
    }

    override fun update(e: AnActionEvent) { e.presentation.isEnabled = e.project != null }
}
