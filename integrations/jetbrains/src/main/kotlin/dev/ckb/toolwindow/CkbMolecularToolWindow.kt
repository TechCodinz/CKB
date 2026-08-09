package dev.ckb.toolwindow

import com.google.gson.JsonObject
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.JBColor
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBList
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.components.JBTabbedPane
import com.intellij.ui.components.JBTextArea
import com.intellij.ui.components.JBTextField
import com.intellij.ui.content.ContentFactory
import com.intellij.util.ui.JBUI
import dev.ckb.api.CkbIntelligenceClient
import java.awt.BorderLayout
import java.awt.Color
import java.awt.Cursor
import java.awt.FlowLayout
import java.awt.Font
import java.awt.GridLayout
import java.io.File
import javax.swing.BorderFactory
import javax.swing.Box
import javax.swing.BoxLayout
import javax.swing.DefaultListModel
import javax.swing.JButton
import javax.swing.JPanel
import javax.swing.JProgressBar
import javax.swing.SwingUtilities

class CkbMolecularToolWindowFactory : ToolWindowFactory {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = CkbMolecularPanel(project)
        val content = ContentFactory.getInstance().createContent(panel, "Invisible Reality V5", false)
        toolWindow.contentManager.addContent(content)
    }
}

data class MolecularNode(
    val id: String,
    val name: String,
    val path: String,
    val role: String,
    val runtimeObserved: Boolean,
    val fanIn: Int,
    val fanOut: Int,
    val activity: Double,
    val changeSensitivity: Double
) {
    override fun toString(): String = name
}

class CkbMolecularPanel(private val project: Project) : JPanel(BorderLayout()) {
    private val cyan = JBColor(Color(0, 128, 150), Color(73, 232, 255))
    private val green = JBColor(Color(18, 140, 90), Color(88, 239, 169))
    private val violet = JBColor(Color(118, 72, 180), Color(201, 156, 255))
    private val amber = JBColor(Color(160, 96, 20), Color(255, 196, 111))
    private val surface = JBColor(Color(247, 250, 253), Color(7, 13, 24))
    private val borderColor = JBColor(Color(205, 217, 229), Color(31, 50, 68))
    private val muted = JBColor(Color(85, 100, 120), Color(126, 145, 171))

    private val status = JBLabel("Invisible architecture memory awaiting hydration")
    private val progress = JProgressBar().apply { isIndeterminate = true; isVisible = false }
    private val analyzeButton = JButton("◉ Deep Analyze")
    private val intentButtons = linkedMapOf<String, JButton>()
    private val lensButtons = linkedMapOf<String, JButton>()
    private val depthButtons = linkedMapOf<String, JButton>()
    private val nodeModel = DefaultListModel<MolecularNode>()
    private val nodeList = JBList(nodeModel)
    private val detail = JBTextArea()
    private val memoryQuery = JBTextField()
    private val memoryResult = JBTextArea()
    private val memoryButton = JButton("Ask Architecture Memory")
    private var bundle: JsonObject? = null
    private var intent = "AUTO"
    private var lens = "SEMANTIC"
    private var depth = "SYSTEM"

    init {
        background = surface
        border = JBUI.Borders.empty(10)
        add(buildHeader(), BorderLayout.NORTH)
        add(buildBody(), BorderLayout.CENTER)
        analyzeButton.addActionListener { analyze() }
        memoryButton.addActionListener { queryMemory() }
        memoryQuery.addActionListener { queryMemory() }
        nodeList.addListSelectionListener { if (!it.valueIsAdjusting) showSelected() }
        nodeList.addMouseListener(object : java.awt.event.MouseAdapter() {
            override fun mouseClicked(e: java.awt.event.MouseEvent) {
                if (e.clickCount == 2) openSelectedSource()
            }
        })
    }

    private fun buildHeader(): JPanel {
        val titleBox = JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS)
            isOpaque = false
            add(JBLabel("●  CKB INVISIBLE REALITY V5").apply { foreground = cyan; font = font.deriveFont(Font.BOLD, 11f) })
            add(Box.createVerticalStrut(4))
            add(JBLabel("Molecular Software Microscope").apply { font = font.deriveFont(Font.BOLD, 18f) })
            add(Box.createVerticalStrut(3))
            add(status.apply { foreground = muted })
            add(Box.createVerticalStrut(3))
            add(JBLabel("STATIC • RUNTIME • PREDICTED remain evidence-separated").apply { foreground = muted; font = font.deriveFont(Font.BOLD, 9f) })
        }
        return JPanel(BorderLayout()).apply {
            background = surface
            border = BorderFactory.createCompoundBorder(BorderFactory.createLineBorder(borderColor), JBUI.Borders.empty(12))
            add(titleBox, BorderLayout.CENTER)
            add(JPanel(FlowLayout(FlowLayout.RIGHT, 4, 0)).apply { isOpaque = false; add(analyzeButton) }, BorderLayout.EAST)
            add(progress, BorderLayout.SOUTH)
        }
    }

    private fun buttonRail(values: List<String>, target: LinkedHashMap<String, JButton>, onClick: (String) -> Unit): JPanel {
        return JPanel(FlowLayout(FlowLayout.LEFT, 5, 2)).apply {
            isOpaque = false
            values.forEach { value ->
                val button = JButton(value)
                target[value] = button
                button.addActionListener { onClick(value); refreshView() }
                add(button)
            }
        }
    }

    private fun buildBody(): JBTabbedPane {
        return JBTabbedPane().apply {
            border = JBUI.Borders.emptyTop(8)
            addTab("Microscope", buildMicroscope())
            addTab("Memory", buildMemory())
            addTab("Evidence", buildEvidence())
        }
    }

    private fun buildMicroscope(): JPanel {
        nodeList.fixedCellHeight = 46
        val controls = JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS)
            isOpaque = false
            add(JBLabel("HYBRID INTELLIGENT INTENT").apply { foreground = violet; font = font.deriveFont(Font.BOLD, 9f) })
            add(buttonRail(listOf("AUTO", "FUSED", "LIVE", "FAULT", "CHANGE", "MEMORY"), intentButtons) { intent = it })
            add(JBLabel("INVISIBLE REALITY LENS").apply { foreground = cyan; font = font.deriveFont(Font.BOLD, 9f) })
            add(buttonRail(listOf("SEMANTIC", "MOLECULE", "NANOTRACE", "STATE"), lensButtons) { lens = it })
            add(JBLabel("SEMANTIC DEPTH").apply { foreground = amber; font = font.deriveFont(Font.BOLD, 9f) })
            add(buttonRail(listOf("SYSTEM", "SUBSYSTEM", "FILE", "SYMBOL", "CALL", "RUNTIME"), depthButtons) { depth = it })
        }
        detail.apply {
            isEditable = false
            lineWrap = true
            wrapStyleWord = true
            background = surface
            foreground = muted
            border = JBUI.Borders.empty(8)
            text = "Run Deep Analyze to reveal architecture activity, hidden callers/callees, change pressure and runtime-observed paths."
        }
        val split = JPanel(GridLayout(1, 2, 8, 0)).apply {
            isOpaque = false
            add(JBScrollPane(nodeList).apply { border = BorderFactory.createLineBorder(borderColor) })
            add(JBScrollPane(detail).apply { border = BorderFactory.createLineBorder(borderColor) })
        }
        return JPanel(BorderLayout(0, 8)).apply {
            border = JBUI.Borders.empty(8, 2)
            isOpaque = false
            add(controls, BorderLayout.NORTH)
            add(split, BorderLayout.CENTER)
        }
    }

    private fun buildMemory(): JPanel {
        memoryQuery.emptyText.text = "Ask about a service, flow, symbol, responsibility or change risk…"
        memoryResult.apply {
            isEditable = false
            lineWrap = true
            wrapStyleWord = true
            background = surface
            font = Font(Font.MONOSPACED, Font.PLAIN, 11)
            border = JBUI.Borders.empty(10)
        }
        return JPanel(BorderLayout(0, 8)).apply {
            border = JBUI.Borders.empty(8, 2)
            isOpaque = false
            add(JPanel(BorderLayout(6, 0)).apply { isOpaque = false; add(memoryQuery, BorderLayout.CENTER); add(memoryButton, BorderLayout.EAST) }, BorderLayout.NORTH)
            add(JBScrollPane(memoryResult).apply { border = BorderFactory.createLineBorder(borderColor) }, BorderLayout.CENTER)
        }
    }

    private fun buildEvidence(): JPanel {
        val area = JBTextArea(
            "CKB Invisible Reality never animates a static dependency as observed execution.\n\n" +
                "SEMANTIC reconstructs architecture by software meaning.\n" +
                "MOLECULE exposes data/request transmission relationships without revealing raw secrets.\n" +
                "NANOTRACE requires runtime evidence.\n" +
                "STATE highlights change sensitivity and observed fault evidence.\n\n" +
                "A proposed change remains PREDICTED until validation and actual source mutation occur."
        ).apply { isEditable = false; lineWrap = true; wrapStyleWord = true; background = surface; foreground = muted; border = JBUI.Borders.empty(12) }
        return JPanel(BorderLayout()).apply { border = JBUI.Borders.empty(8, 2); isOpaque = false; add(JBScrollPane(area), BorderLayout.CENTER) }
    }

    private fun setBusy(value: Boolean, message: String) {
        progress.isVisible = value
        analyzeButton.isEnabled = !value
        memoryButton.isEnabled = !value
        cursor = if (value) Cursor.getPredefinedCursor(Cursor.WAIT_CURSOR) else Cursor.getDefaultCursor()
        status.text = message
    }

    private fun analyze() {
        val root = project.basePath ?: return
        setBusy(true, "Reconstructing hidden architecture reality…")
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val next = CkbIntelligenceClient.bundle(root)
                SwingUtilities.invokeLater {
                    bundle = next
                    refreshView()
                    setBusy(false, "Invisible Reality hydrated from local CKB Core")
                }
            } catch (e: Exception) {
                SwingUtilities.invokeLater { setBusy(false, "Deep intelligence unavailable: ${e.message}") }
            }
        }
    }

    private fun refreshView() {
        val activity = bundle?.getAsJsonObject("activity")
        val hotspots = activity?.getAsJsonArray("hotspots")
        nodeModel.clear()
        val candidates = mutableListOf<MolecularNode>()
        hotspots?.forEach { element ->
            val n = element.asJsonObject
            val item = MolecularNode(
                id = n.get("id")?.asString ?: "",
                name = n.get("name")?.asString ?: n.get("id")?.asString ?: "symbol",
                path = n.get("path")?.asString ?: "",
                role = n.get("role")?.asString ?: "architecture-symbol",
                runtimeObserved = n.get("runtimeObserved")?.asBoolean ?: false,
                fanIn = n.get("fanIn")?.asInt ?: 0,
                fanOut = n.get("fanOut")?.asInt ?: 0,
                activity = n.get("activityIndex")?.asDouble ?: 0.0,
                changeSensitivity = n.get("changeSensitivityIndex")?.asDouble ?: 0.0
            )
            val resolvedIntent = if (intent == "AUTO") {
                if (item.runtimeObserved) "LIVE" else "FUSED"
            } else intent
            val keep = when {
                lens == "NANOTRACE" || depth == "RUNTIME" || resolvedIntent == "LIVE" -> item.runtimeObserved
                lens == "STATE" || resolvedIntent == "FAULT" -> item.changeSensitivity >= 0.45
                depth == "CALL" -> item.fanIn + item.fanOut > 0
                else -> true
            }
            if (keep) candidates.add(item)
        }
        candidates.sortedByDescending { maxOf(it.activity, it.changeSensitivity) }.take(60).forEach { nodeModel.addElement(it) }
        val runtimeCount = candidates.count { it.runtimeObserved }
        detail.text = "${lens} • ${depth} • ${intent}\n\n${candidates.size} evidence-backed symbols match this lens. ${runtimeCount} carry runtime observation. Double-click a symbol to open its source.\n\nThe view is derived from local CKB architecture memory; unavailable runtime data is never synthesized."
        updateButtons()
    }

    private fun updateButtons() {
        intentButtons.forEach { (key, button) -> button.foreground = if (key == intent) violet else JBColor.foreground() }
        lensButtons.forEach { (key, button) -> button.foreground = if (key == lens) cyan else JBColor.foreground() }
        depthButtons.forEach { (key, button) -> button.foreground = if (key == depth) amber else JBColor.foreground() }
        val hasRuntime = nodeModel.elements().toList().any { it.runtimeObserved }
        intentButtons["LIVE"]?.isEnabled = hasRuntime || bundle == null
        depthButtons["RUNTIME"]?.isEnabled = hasRuntime || bundle == null
    }

    private fun showSelected() {
        val item = nodeList.selectedValue ?: return
        detail.text = "${item.name}\n${item.path}\n\nRole: ${item.role}\nIncoming: ${item.fanIn}\nOutgoing: ${item.fanOut}\nActivity index: ${String.format("%.2f", item.activity)}\nChange sensitivity: ${String.format("%.2f", item.changeSensitivity)}\nRuntime observed: ${if (item.runtimeObserved) "yes" else "no"}\n\nThis is an evidence-backed navigation view, not a failure probability."
    }

    private fun openSelectedSource() {
        val item = nodeList.selectedValue ?: return
        val root = project.basePath ?: return
        val raw = item.path.ifBlank { item.id.substringBefore("::") }
        val file = if (File(raw).isAbsolute) File(raw) else File(root, raw)
        val virtualFile = LocalFileSystem.getInstance().findFileByIoFile(file) ?: return
        FileEditorManager.getInstance(project).openFile(virtualFile, true)
    }

    private fun queryMemory() {
        val root = project.basePath ?: return
        val query = memoryQuery.text.trim()
        if (query.isBlank()) return
        setBusy(true, "Retrieving bounded architecture memory…")
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val response = CkbIntelligenceClient.memory(root, query)
                val context = response.getAsJsonObject("memory")?.get("context")?.asString ?: "No architecture context returned."
                SwingUtilities.invokeLater { memoryResult.text = context; memoryResult.caretPosition = 0; setBusy(false, "Architecture memory retrieved") }
            } catch (e: Exception) {
                SwingUtilities.invokeLater { setBusy(false, "Memory unavailable: ${e.message}") }
            }
        }
    }
}
