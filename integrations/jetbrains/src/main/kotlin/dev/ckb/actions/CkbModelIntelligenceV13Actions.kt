package dev.ckb.actions

import com.google.gson.GsonBuilder
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
        V13_TASKS.map { it.uppercase() }.toTypedArray(),
        V13_TASKS[0].uppercase(),
        null,
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
        background(e, "CKB V13 • Compile Architecture Context") {
            CkbModelIntelligenceV13.compileContext("current", query, task, cursor.path, cursor.line, cursor.symbol)
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

class ShowArchitectureConstitutionV13Action : AnAction("Show Architecture Constitution") {
    override fun actionPerformed(e: AnActionEvent) {
        background(e, "CKB V13 • Architecture Constitution") { CkbModelIntelligenceV13.constitution() }
    }

    override fun update(e: AnActionEvent) { e.presentation.isEnabled = e.project != null }
}
