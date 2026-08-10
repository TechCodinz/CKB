package dev.ckb.actions

import com.intellij.ide.BrowserUtil
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiNamedElement
import java.io.File
import java.net.URLEncoder
import java.nio.charset.StandardCharsets

private const val CKB_CLOUD_EXPLORER = "https://ckb-nu.vercel.app/project/current"

private data class JetBrainsCursorReality(
    val file: String,
    val line: Int,
    val column: Int,
    val symbol: String,
    val depth: String
)

private fun encode(value: String): String = URLEncoder.encode(value, StandardCharsets.UTF_8)

private fun cursorReality(e: AnActionEvent): JetBrainsCursorReality? {
    val project = e.project ?: return null
    val editor = FileEditorManager.getInstance(project).selectedTextEditor ?: return null
    val virtualFile = FileDocumentManager.getInstance().getFile(editor.document) ?: return null
    val root = project.basePath
    val relative = if (root.isNullOrBlank()) {
        virtualFile.path
    } else {
        try {
            File(root).toPath().relativize(File(virtualFile.path).toPath()).toString()
        } catch (_: Exception) {
            virtualFile.path
        }
    }.replace('\\', '/')

    val offset = editor.caretModel.offset.coerceIn(0, maxOf(0, editor.document.textLength - 1))
    val psiFile = PsiDocumentManager.getInstance(project).getPsiFile(editor.document)
    var element = if (psiFile != null && editor.document.textLength > 0) psiFile.findElementAt(offset) else null
    var symbol = ""
    while (element != null) {
        if (element is PsiNamedElement && !element.name.isNullOrBlank()) {
            symbol = element.name.orEmpty()
            break
        }
        element = element.parent
    }

    return JetBrainsCursorReality(
        file = relative,
        line = editor.caretModel.logicalPosition.line + 1,
        column = editor.caretModel.logicalPosition.column + 1,
        symbol = symbol,
        depth = when {
            editor.selectionModel.hasSelection() -> "line"
            symbol.isNotBlank() -> "symbol"
            else -> "file"
        }
    )
}

private fun openCloudReality(e: AnActionEvent, openRaiziom: Boolean) {
    val cursor = cursorReality(e) ?: return
    val query = mutableListOf(
        "from=jetbrains",
        "experience=semantic-editor-v10",
        "tab=0",
        "file=${encode(cursor.file)}",
        "line=${cursor.line}",
        "column=${cursor.column}",
        "depth=${encode(cursor.depth)}",
        "resume=xray"
    )
    if (cursor.symbol.isNotBlank()) query += "symbol=${encode(cursor.symbol.take(180))}"
    if (openRaiziom) query += "raiziom=1"
    BrowserUtil.browse("$CKB_CLOUD_EXPLORER?${query.joinToString("&")}")
}

class ContinueSemanticRealityInCloudAction : AnAction("Continue Cursor Reality in CKB Cloud") {
    override fun actionPerformed(e: AnActionEvent) = openCloudReality(e, false)

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.project != null && FileEditorManager.getInstance(e.project!!).selectedTextEditor != null
    }
}

class AskRaiziomAboutCursorAction : AnAction("Ask Raiziom About Cursor Reality") {
    override fun actionPerformed(e: AnActionEvent) = openCloudReality(e, true)

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.project != null && FileEditorManager.getInstance(e.project!!).selectedTextEditor != null
    }
}
