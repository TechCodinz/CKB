package dev.ckb.toolwindow

import com.google.gson.JsonObject
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiNamedElement
import com.intellij.ui.JBColor
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.components.JBTextArea
import com.intellij.ui.content.ContentFactory
import com.intellij.util.ui.JBUI
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import dev.ckb.api.CkbIntelligenceClient
import dev.ckb.api.CkbRuntimeRealityClient
import java.awt.BorderLayout
import java.awt.Color
import java.awt.FlowLayout
import java.awt.Font
import java.io.File
import javax.swing.BorderFactory
import javax.swing.Box
import javax.swing.BoxLayout
import javax.swing.JButton
import javax.swing.JPanel
import javax.swing.SwingUtilities
import javax.swing.Timer
import kotlin.math.max

class CkbSemanticEditorRealityToolWindowFactory : ToolWindowFactory {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = CkbSemanticEditorRealityPanel(project)
        toolWindow.contentManager.addContent(
            ContentFactory.getInstance().createContent(panel, "Semantic Editor V10", false)
        )
    }
}

private data class EditorRuntimeHop(
    val traceId: String,
    val stepIndex: Int,
    val stepCount: Int,
    val source: String,
    val target: String,
    val operation: String,
    val flowType: String,
    val durationMs: Double,
    val error: Boolean,
    val role: String
)

private data class EditorRealitySnapshot(
    val system: String,
    val subsystem: String,
    val file: String,
    val line: Int,
    val column: Int,
    val symbol: String,
    val symbolKind: String,
    val depth: String,
    val depthMode: String,
    val fanIn: Int,
    val fanOut: Int,
    val activity: Double,
    val changeSensitivity: Double,
    val runtimeObservedByActivity: Boolean,
    val exactHop: EditorRuntimeHop?
)

private fun slash(value: String): String = value.replace('\\', '/').removePrefix("./").trimStart('/')

private fun sameFile(a: String, b: String): Boolean {
    val x = slash(a).lowercase()
    val y = slash(b).lowercase()
    if (x.isBlank() || y.isBlank()) return false
    return x == y || x.endsWith("/$y") || y.endsWith("/$x")
}

private fun identityFile(value: String): String = slash(value.substringBefore("::"))
private fun identitySymbol(value: String): String = if (value.contains("::")) value.substringAfterLast("::") else ""

private fun subsystemFor(relativeFile: String): String {
    val ignored = setOf("src", "app", "apps", "lib", "libs", "packages", "pkg", "source")
    val parts = slash(relativeFile).split('/').filter { it.isNotBlank() }
    val meaningful = parts.dropLast(1).filter { it.lowercase() !in ignored }
    return when {
        meaningful.isNotEmpty() -> meaningful.take(2).joinToString("/")
        parts.size > 1 -> parts.first()
        else -> "workspace-root"
    }
}

private fun normalizedFlow(step: JsonObject): String {
    val text = listOf(
        step.get("flowType")?.asString.orEmpty(),
        step.get("operation")?.asString.orEmpty(),
        step.get("protocol")?.asString.orEmpty(),
        step.get("dbSystem")?.asString.orEmpty(),
        step.get("messagingSystem")?.asString.orEmpty()
    ).joinToString(" ").lowercase()
    return when {
        Regex("websocket|\\bws\\b|\\bwss\\b").containsMatchIn(text) -> "websocket"
        Regex("redis|cache").containsMatchIn(text) -> "cache"
        Regex("postgres|mysql|sqlite|mongo|prisma|database|\\bsql\\b").containsMatchIn(text) -> "database"
        Regex("queue|kafka|rabbit|bull|sqs|pubsub|message").containsMatchIn(text) -> "queue"
        Regex("event").containsMatchIn(text) -> "event"
        Regex("http|rpc|fetch|request|response").containsMatchIn(text) -> "http"
        else -> "function"
    }
}

class CkbSemanticEditorRealityPanel(private val project: Project) : JPanel(BorderLayout()) {
    private val cyan = JBColor(Color(0, 128, 150), Color(73, 232, 255))
    private val green = JBColor(Color(18, 140, 90), Color(88, 239, 169))
    private val violet = JBColor(Color(118, 72, 180), Color(201, 156, 255))
    private val amber = JBColor(Color(160, 96, 20), Color(255, 196, 111))
    private val red = JBColor(Color(190, 55, 75), Color(255, 117, 142))
    private val muted = JBColor(Color(85, 100, 120), Color(126, 145, 171))
    private val surface = JBColor(Color(247, 250, 253), Color(7, 13, 24))
    private val borderColor = JBColor(Color(205, 217, 229), Color(31, 50, 68))

    private val status = JBLabel("Move the cursor through source to enter semantic reality")
    private val breadcrumb = JBLabel("SYSTEM → SUBSYSTEM → FILE → SYMBOL → CALL → LINE")
    private val detail = JBTextArea()
    private val analyzeButton = JButton("◉ Deep Analyze")
    private val runtimeButton = JButton("↻ Live Reality")
    private val depthButtons = linkedMapOf<String, JButton>()
    private var bundle: JsonObject? = null
    private var traces: JsonObject? = null
    private var runtimeOnline = false
    private var replaySafe = false
    private var manualDepth: String? = null
    private var analyzing = false
    private var runtimeRefreshing = false
    private var lastSignature = ""

    private val editorTimer = Timer(650) { if (isShowing) refreshEditorReality() }
    private val runtimeTimer = Timer(3000) { if (isShowing) refreshRuntime(true) }

    init {
        background = surface
        border = JBUI.Borders.empty(10)
        add(buildHeader(), BorderLayout.NORTH)
        add(buildBody(), BorderLayout.CENTER)
        analyzeButton.addActionListener { analyze() }
        runtimeButton.addActionListener { refreshRuntime(false) }
        editorTimer.start()
        runtimeTimer.start()
        analyze()
        refreshRuntime(true)
    }

    private fun buildHeader(): JPanel {
        val copy = JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS)
            isOpaque = false
            add(JBLabel("●  CKB CURSOR-DRIVEN SEMANTIC REALITY V10").apply { foreground = cyan; font = font.deriveFont(Font.BOLD, 11f) })
            add(Box.createVerticalStrut(4))
            add(JBLabel("Editor viewport becomes a software-reality microscope").apply { font = font.deriveFont(Font.BOLD, 17f) })
            add(Box.createVerticalStrut(3))
            add(status.apply { foreground = muted })
            add(Box.createVerticalStrut(3))
            add(breadcrumb.apply { foreground = violet; font = font.deriveFont(Font.BOLD, 9f) })
        }
        return JPanel(BorderLayout()).apply {
            isOpaque = false
            border = BorderFactory.createCompoundBorder(BorderFactory.createLineBorder(borderColor), JBUI.Borders.empty(12))
            add(copy, BorderLayout.CENTER)
            add(JPanel(FlowLayout(FlowLayout.RIGHT, 4, 0)).apply {
                isOpaque = false
                add(runtimeButton)
                add(analyzeButton)
            }, BorderLayout.EAST)
        }
    }

    private fun buildBody(): JPanel {
        val depthRail = JPanel(FlowLayout(FlowLayout.LEFT, 5, 3)).apply {
            isOpaque = false
            listOf("AUTO", "LINE", "CALL", "SYMBOL", "FILE", "SUBSYSTEM", "SYSTEM").forEach { value ->
                val button = JButton(value)
                depthButtons[value] = button
                button.addActionListener {
                    manualDepth = if (value == "AUTO") null else value
                    refreshEditorReality(force = true)
                }
                add(button)
            }
        }
        detail.apply {
            isEditable = false
            lineWrap = true
            wrapStyleWord = true
            background = surface
            foreground = muted
            border = JBUI.Borders.empty(12)
            font = Font(Font.MONOSPACED, Font.PLAIN, 11)
            text = "CKB is waiting for a source editor. Runtime evidence is shown only when exact observed parent/child spans map back to the current file/symbol."
        }
        val truth = JBLabel("STATIC anatomy ≠ RUNTIME execution ≠ PREDICTED impact").apply {
            foreground = amber
            font = font.deriveFont(Font.BOLD, 9f)
            border = JBUI.Borders.empty(4, 2)
        }
        return JPanel(BorderLayout(0, 8)).apply {
            isOpaque = false
            border = JBUI.Borders.emptyTop(8)
            add(depthRail, BorderLayout.NORTH)
            add(JBScrollPane(detail).apply { border = BorderFactory.createLineBorder(borderColor) }, BorderLayout.CENTER)
            add(truth, BorderLayout.SOUTH)
        }
    }

    private fun analyze() {
        val root = project.basePath ?: return
        if (analyzing) return
        analyzing = true
        analyzeButton.isEnabled = false
        status.text = "Reconstructing local architecture activity…"
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val next = CkbIntelligenceClient.bundle(root)
                SwingUtilities.invokeLater {
                    bundle = next
                    analyzing = false
                    analyzeButton.isEnabled = true
                    refreshEditorReality(force = true)
                }
            } catch (e: Exception) {
                SwingUtilities.invokeLater {
                    analyzing = false
                    analyzeButton.isEnabled = true
                    status.text = "Deep local intelligence unavailable • source semantics remain visible"
                    detail.text = "CKB local deep analysis is unavailable: ${e.message}\n\nThe semantic editor can still resolve PSI/source hierarchy. Runtime remains separate and is never synthesized."
                }
            }
        }
    }

    private fun refreshRuntime(quiet: Boolean) {
        if (runtimeRefreshing) return
        runtimeRefreshing = true
        if (!quiet) status.text = "Reading exact observed runtime traces…"
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val (traceData, _) = CkbRuntimeRealityClient.snapshot()
                SwingUtilities.invokeLater {
                    traces = traceData.getAsJsonObject("traces")
                    replaySafe = traceData.get("replaySafe")?.asBoolean == true && traceData.get("traceSemantics")?.asString == "exact-observed-span-instances"
                    runtimeOnline = true
                    runtimeRefreshing = false
                    refreshEditorReality(force = true)
                }
            } catch (_: Exception) {
                SwingUtilities.invokeLater {
                    runtimeOnline = false
                    replaySafe = false
                    runtimeRefreshing = false
                    refreshEditorReality(force = true)
                }
            }
        }
    }

    private fun relativePath(filePath: String): String {
        val root = project.basePath ?: return slash(filePath)
        return try {
            slash(File(root).toPath().relativize(File(filePath).toPath()).toString())
        } catch (_: Exception) {
            slash(filePath)
        }
    }

    private fun editorSymbol(): Triple<String, String, IntRange>? {
        val editor = FileEditorManager.getInstance(project).selectedTextEditor ?: return null
        val psiFile = PsiDocumentManager.getInstance(project).getPsiFile(editor.document) ?: return null
        val offset = editor.caretModel.offset.coerceIn(0, max(0, editor.document.textLength - 1))
        var current = psiFile.findElementAt(offset)
        while (current != null) {
            if (current is PsiNamedElement && !current.name.isNullOrBlank() && current.textRange != null) {
                return Triple(current.name.orEmpty(), current.javaClass.simpleName, current.textRange.startOffset..current.textRange.endOffset)
            }
            current = current.parent
        }
        return null
    }

    private fun autoDepth(editorVisibleLines: Int, hasSelection: Boolean, hasSymbol: Boolean): String = when {
        hasSelection -> "LINE"
        hasSymbol && editorVisibleLines <= 38 -> "CALL"
        hasSymbol && editorVisibleLines <= 90 -> "SYMBOL"
        editorVisibleLines <= 220 -> "FILE"
        editorVisibleLines <= 520 -> "SUBSYSTEM"
        else -> "SYSTEM"
    }

    private fun hotspot(relativeFile: String, symbol: String): JsonObject? {
        val activity = bundle?.getAsJsonObject("activity") ?: return null
        val hotspots = activity.getAsJsonArray("hotspots") ?: return null
        var fileFallback: JsonObject? = null
        hotspots.forEach { element ->
            val node = element.asJsonObject
            val nodePath = slash(node.get("path")?.asString ?: node.get("id")?.asString?.substringBefore("::").orEmpty())
            if (!sameFile(nodePath, relativeFile)) return@forEach
            if (fileFallback == null) fileFallback = node
            val nodeName = node.get("name")?.asString ?: node.get("id")?.asString?.substringAfterLast("::").orEmpty()
            if (symbol.isNotBlank() && nodeName == symbol) return node
        }
        return fileFallback
    }

    private fun exactHop(relativeFile: String, symbol: String): EditorRuntimeHop? {
        if (!replaySafe) return null
        val traceObject = traces ?: return null
        val candidates = mutableListOf<EditorRuntimeHop>()
        traceObject.entrySet().forEach { (traceId, raw) ->
            if (!raw.isJsonArray) return@forEach
            val rows = raw.asJsonArray
            rows.forEachIndexed { index, element ->
                val step = element.asJsonObject
                val source = step.get("source")?.asString.orEmpty()
                val target = step.get("target")?.asString.orEmpty()
                val sourceMatches = sameFile(identityFile(source), relativeFile) && (symbol.isBlank() || identitySymbol(source).isBlank() || identitySymbol(source) == symbol)
                val targetMatches = sameFile(identityFile(target), relativeFile) && (symbol.isBlank() || identitySymbol(target).isBlank() || identitySymbol(target) == symbol)
                if (!sourceMatches && !targetMatches) return@forEachIndexed
                candidates.add(EditorRuntimeHop(
                    traceId = traceId,
                    stepIndex = index,
                    stepCount = rows.size(),
                    source = source,
                    target = target,
                    operation = step.get("operation")?.asString ?: "observed transition",
                    flowType = normalizedFlow(step),
                    durationMs = step.get("durationMs")?.asDouble ?: 0.0,
                    error = step.get("error")?.asBoolean ?: false,
                    role = if (targetMatches) "TARGET" else "SOURCE"
                ))
            }
        }
        return candidates.sortedWith(compareByDescending<EditorRuntimeHop> { it.error }.thenByDescending { it.durationMs }).firstOrNull()
    }

    private fun refreshEditorReality(force: Boolean = false) {
        val editor = FileEditorManager.getInstance(project).selectedTextEditor
        if (editor == null) {
            status.text = "Open a source editor to enter semantic reality"
            detail.text = "No active source editor. CKB will not invent a source target."
            return
        }
        val virtualFile = FileDocumentManager.getInstance().getFile(editor.document)
        if (virtualFile == null) {
            status.text = "Current editor is not backed by a source file"
            return
        }
        val relativeFile = relativePath(virtualFile.path)
        val symbolReality = editorSymbol()
        val symbol = symbolReality?.first.orEmpty()
        val symbolKind = symbolReality?.second.orEmpty()
        val line = editor.caretModel.logicalPosition.line + 1
        val column = editor.caretModel.logicalPosition.column + 1
        val visibleLines = max(1, editor.scrollingModel.visibleArea.height / max(1, editor.lineHeight))
        val depth = manualDepth ?: autoDepth(visibleLines, editor.selectionModel.hasSelection(), symbol.isNotBlank())
        val hot = hotspot(relativeFile, symbol)
        val hop = exactHop(relativeFile, symbol)
        val system = project.name.ifBlank { File(project.basePath ?: "workspace").name }
        val subsystem = subsystemFor(relativeFile)
        val snapshot = EditorRealitySnapshot(
            system = system,
            subsystem = subsystem,
            file = relativeFile,
            line = line,
            column = column,
            symbol = symbol,
            symbolKind = symbolKind,
            depth = depth,
            depthMode = if (manualDepth == null) "AUTO" else "MANUAL",
            fanIn = hot?.get("fanIn")?.asInt ?: 0,
            fanOut = hot?.get("fanOut")?.asInt ?: 0,
            activity = hot?.get("activityIndex")?.asDouble ?: 0.0,
            changeSensitivity = hot?.get("changeSensitivityIndex")?.asDouble ?: 0.0,
            runtimeObservedByActivity = hot?.get("runtimeObserved")?.asBoolean ?: false,
            exactHop = hop
        )
        val signature = listOf(relativeFile, line, column, symbol, depth, hop?.traceId, hop?.stepIndex, replaySafe, runtimeOnline).joinToString("|")
        if (!force && signature == lastSignature) return
        lastSignature = signature
        render(snapshot)
    }

    private fun render(snapshot: EditorRealitySnapshot) {
        val hop = snapshot.exactHop
        status.text = when {
            hop != null -> "LIVE ${hop.flowType.uppercase()} • ${hop.durationMs.format(2)} ms • ${snapshot.depth}"
            runtimeOnline -> "${snapshot.depth} reality • runtime online, no exact hop at cursor"
            else -> "${snapshot.depth} reality • static source + architecture"
        }
        breadcrumb.text = listOf(
            snapshot.system,
            snapshot.subsystem,
            snapshot.file,
            snapshot.symbol.ifBlank { "cursor:${snapshot.line}" },
            snapshot.depth
        ).joinToString("  →  ")
        depthButtons.forEach { (key, button) ->
            val active = if (key == "AUTO") snapshot.depthMode == "AUTO" else key == snapshot.depth
            button.foreground = if (active) cyan else JBColor.foreground()
        }

        val lines = mutableListOf<String>()
        lines += "CKB SEMANTIC EDITOR REALITY V10"
        lines += ""
        lines += "Depth        : ${snapshot.depth} (${snapshot.depthMode.lowercase()})"
        lines += "System       : ${snapshot.system}"
        lines += "Subsystem    : ${snapshot.subsystem}"
        lines += "File         : ${snapshot.file}"
        lines += "Cursor       : ${snapshot.line}:${snapshot.column}"
        if (snapshot.symbol.isNotBlank()) lines += "Symbol       : ${snapshot.symbol}  [${snapshot.symbolKind}]"
        lines += ""
        lines += "STATIC ARCHITECTURE"
        lines += "Incoming     : ${snapshot.fanIn}"
        lines += "Outgoing     : ${snapshot.fanOut}"
        lines += "Activity     : ${snapshot.activity.format(2)}"
        lines += "Change sens. : ${(snapshot.changeSensitivity * 100).format(0)}%"
        lines += "Runtime flag : ${if (snapshot.runtimeObservedByActivity) "observed in architecture activity" else "not observed at this activity node"}"
        lines += ""
        if (hop != null) {
            lines += "EXACT OBSERVED RUNTIME"
            lines += "Role         : ${hop.role}"
            lines += "Trace        : ${hop.traceId}"
            lines += "Step         : ${hop.stepIndex + 1}/${hop.stepCount}"
            lines += "Flow         : ${hop.flowType.uppercase()}"
            lines += "Duration     : ${hop.durationMs.format(2)} ms"
            lines += "Error        : ${if (hop.error) "OBSERVED" else "no error flag on this hop"}"
            lines += "Source       : ${hop.source}"
            lines += "Target       : ${hop.target}"
            lines += "Operation    : ${hop.operation}"
            lines += ""
            lines += "The runtime block above exists only because exact observed parent/child span semantics mapped to this file/symbol."
        } else {
            lines += "RUNTIME"
            lines += when {
                !runtimeOnline -> "Reality server unavailable. Static source intelligence remains active."
                !replaySafe -> "Runtime may exist, but exact replay-safe parent/child span semantics are not available."
                else -> "Exact traces exist elsewhere, but no observed hop maps to the current cursor target."
            }
        }
        lines += ""
        lines += "TRUTH CONTRACT"
        lines += "STATIC source/architecture is not presented as execution. PREDICTED change impact remains separate from both."
        detail.text = lines.joinToString("\n")
        detail.caretPosition = 0
    }

    private fun Double.format(decimals: Int): String = String.format("%.${decimals}f", this)

    override fun removeNotify() {
        editorTimer.stop()
        runtimeTimer.stop()
        super.removeNotify()
    }

    override fun addNotify() {
        super.addNotify()
        if (!editorTimer.isRunning) editorTimer.start()
        if (!runtimeTimer.isRunning) runtimeTimer.start()
    }
}
