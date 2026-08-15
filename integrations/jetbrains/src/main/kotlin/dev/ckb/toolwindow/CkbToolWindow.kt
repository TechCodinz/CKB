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
import dev.ckb.api.CkbApiClient
import dev.ckb.api.CkbIntelligenceClient
import dev.ckb.api.ScanReport
import java.awt.BorderLayout
import java.awt.Color
import java.awt.Component
import java.awt.Cursor
import java.awt.Dimension
import java.awt.FlowLayout
import java.awt.Font
import java.awt.GridLayout
import java.io.File
import javax.swing.BorderFactory
import javax.swing.Box
import javax.swing.BoxLayout
import javax.swing.DefaultListCellRenderer
import javax.swing.DefaultListModel
import javax.swing.JButton
import javax.swing.JList
import javax.swing.JPanel
import javax.swing.JProgressBar
import javax.swing.SwingConstants
import javax.swing.SwingUtilities

class CkbToolWindowFactory : ToolWindowFactory {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = CkbToolWindowPanel(project)
        val content = ContentFactory.getInstance().createContent(panel, "Living Reality", false)
        toolWindow.contentManager.addContent(content)
    }
}

data class ActivityItem(
    val id: String,
    val name: String,
    val path: String,
    val role: String,
    val activity: Double,
    val changeSensitivity: Double,
    val runtimeObserved: Boolean,
    val fanIn: Int,
    val fanOut: Int
) {
    override fun toString(): String = name
}

class CkbToolWindowPanel(private val project: Project) : JPanel(BorderLayout()) {
    private val cyan = JBColor(Color(0, 125, 145), Color(67, 233, 255))
    private val green = JBColor(Color(16, 135, 83), Color(80, 242, 164))
    private val purple = JBColor(Color(116, 71, 173), Color(194, 140, 255))
    private val amber = JBColor(Color(155, 91, 20), Color(255, 189, 102))
    private val surface = JBColor(Color(248, 250, 253), Color(10, 15, 27))
    private val surfaceStrong = JBColor(Color(238, 243, 249), Color(7, 10, 18))
    private val borderColor = JBColor(Color(205, 217, 229), Color(35, 52, 72))
    private val muted = JBColor(Color(85, 100, 120), Color(128, 144, 170))

    private val statusLabel = JBLabel("Architecture memory awaiting hydration")
    private val sourceLabel = JBLabel("STATIC • RUNTIME • PREDICTED remain separate")
    private val progress = JProgressBar().apply { isIndeterminate = true; isVisible = false }
    private val deepButton = JButton("◉ Deep Analyze")
    private val scanButton = JButton("↻ Base Scan")

    private val symbolMetric = metricValue("—", cyan)
    private val relationMetric = metricValue("—", purple)
    private val runtimeMetric = metricValue("—", green)
    private val dnaMetric = metricValue("—", amber)

    private val activityModel = DefaultListModel<ActivityItem>()
    private val activityList = JBList(activityModel)
    private val findingsModel = DefaultListModel<String>()
    private val findingsList = JBList(findingsModel)
    private val boundaryArea = JBTextArea()
    private val memoryQuery = JBTextField()
    private val memoryButton = JButton("Query Memory")
    private val memoryArea = JBTextArea()

    init {
        border = JBUI.Borders.empty(10)
        background = surfaceStrong
        add(buildHeader(), BorderLayout.NORTH)
        add(buildTabs(), BorderLayout.CENTER)

        deepButton.addActionListener { runDeepAnalysis() }
        scanButton.addActionListener { runBaseScan() }
        memoryButton.addActionListener { queryMemory() }
        memoryQuery.addActionListener { queryMemory() }
        activityList.addListSelectionListener { event ->
            if (!event.valueIsAdjusting) {
                val item = activityList.selectedValue ?: return@addListSelectionListener
                statusLabel.text = "Focused: ${item.name} • ${item.role} • change sensitivity ${(item.changeSensitivity * 100).toInt()}%"
            }
        }
        activityList.addMouseListener(object : java.awt.event.MouseAdapter() {
            override fun mouseClicked(event: java.awt.event.MouseEvent) {
                if (event.clickCount == 2) openActivitySource(activityList.selectedValue)
            }
        })
    }

    private fun buildHeader(): JPanel {
        val outer = JPanel(BorderLayout()).apply {
            background = surface
            border = BorderFactory.createCompoundBorder(
                BorderFactory.createLineBorder(borderColor),
                JBUI.Borders.empty(12)
            )
        }
        val titleBox = JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS)
            isOpaque = false
        }
        val eyebrow = JBLabel("●  CKB LIVING ARCHITECTURE REALITY").apply {
            foreground = cyan
            font = font.deriveFont(Font.BOLD, 11f)
        }
        val title = JBLabel("Software Intelligence Core").apply {
            font = font.deriveFont(Font.BOLD, 18f)
        }
        statusLabel.foreground = muted
        statusLabel.font = statusLabel.font.deriveFont(11f)
        sourceLabel.foreground = muted
        sourceLabel.font = sourceLabel.font.deriveFont(Font.BOLD, 9.5f)
        titleBox.add(eyebrow)
        titleBox.add(Box.createVerticalStrut(4))
        titleBox.add(title)
        titleBox.add(Box.createVerticalStrut(3))
        titleBox.add(statusLabel)
        titleBox.add(Box.createVerticalStrut(3))
        titleBox.add(sourceLabel)

        val actions = JPanel(FlowLayout(FlowLayout.RIGHT, 6, 0)).apply {
            isOpaque = false
            add(scanButton)
            add(deepButton)
        }
        outer.add(titleBox, BorderLayout.CENTER)
        outer.add(actions, BorderLayout.EAST)
        outer.add(progress, BorderLayout.SOUTH)
        return outer
    }

    private fun metricValue(initial: String, color: Color): JBLabel = JBLabel(initial, SwingConstants.LEFT).apply {
        foreground = color
        font = font.deriveFont(Font.BOLD, 20f)
    }

    private fun metricCard(label: String, value: JBLabel): JPanel {
        return JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS)
            background = surface
            border = BorderFactory.createCompoundBorder(
                BorderFactory.createLineBorder(borderColor),
                JBUI.Borders.empty(10)
            )
            add(value)
            add(Box.createVerticalStrut(3))
            add(JBLabel(label).apply {
                foreground = muted
                font = font.deriveFont(Font.BOLD, 9.5f)
            })
        }
    }

    private fun buildRealityTab(): JPanel {
        val root = JPanel(BorderLayout(0, 10)).apply {
            border = JBUI.Borders.empty(10, 2)
            isOpaque = false
        }
        val metrics = JPanel(GridLayout(1, 4, 8, 0)).apply {
            isOpaque = false
            add(metricCard("ARCHITECTURE SYMBOLS", symbolMetric))
            add(metricCard("RELATIONSHIPS", relationMetric))
            add(metricCard("RUNTIME COVERAGE", runtimeMetric))
            add(metricCard("CODE DNA HEALTH", dnaMetric))
        }
        val explainer = JBTextArea(
            "CKB maps the internal software reality without collapsing evidence classes. " +
                "STATIC describes relationships present in code. RUNTIME appears only when telemetry observed execution. " +
                "PREDICTED is reserved for change/impact simulation. Select Activity to inspect hotspots, Memory to retrieve bounded model context, or Findings for current architecture drift."
        ).apply {
            isEditable = false
            lineWrap = true
            wrapStyleWord = true
            background = surface
            foreground = muted
            border = BorderFactory.createCompoundBorder(
                BorderFactory.createLineBorder(borderColor),
                JBUI.Borders.empty(14)
            )
        }
        root.add(metrics, BorderLayout.NORTH)
        root.add(explainer, BorderLayout.CENTER)
        return root
    }

    private fun buildActivityTab(): JPanel {
        activityList.cellRenderer = ActivityCellRenderer(cyan, green, purple, muted, surface)
        activityList.fixedCellHeight = 58
        val listScroll = JBScrollPane(activityList).apply {
            border = BorderFactory.createLineBorder(borderColor)
        }
        boundaryArea.apply {
            isEditable = false
            lineWrap = true
            wrapStyleWord = true
            background = surface
            foreground = muted
            font = Font(Font.MONOSPACED, Font.PLAIN, 11)
            border = JBUI.Borders.empty(8)
        }
        val boundaryScroll = JBScrollPane(boundaryArea).apply {
            preferredSize = Dimension(280, 180)
            border = BorderFactory.createLineBorder(borderColor)
        }
        return JPanel(BorderLayout(8, 0)).apply {
            border = JBUI.Borders.empty(10, 2)
            isOpaque = false
            add(listScroll, BorderLayout.CENTER)
            add(boundaryScroll, BorderLayout.EAST)
        }
    }

    private fun buildMemoryTab(): JPanel {
        memoryArea.apply {
            isEditable = false
            lineWrap = false
            background = surface
            foreground = JBColor.foreground()
            font = Font(Font.MONOSPACED, Font.PLAIN, 11)
            border = JBUI.Borders.empty(10)
        }
        memoryQuery.emptyText.text = "Ask about a symbol, flow, service, responsibility or risk…"
        val queryRow = JPanel(BorderLayout(6, 0)).apply {
            isOpaque = false
            add(memoryQuery, BorderLayout.CENTER)
            add(memoryButton, BorderLayout.EAST)
        }
        return JPanel(BorderLayout(0, 8)).apply {
            border = JBUI.Borders.empty(10, 2)
            isOpaque = false
            add(queryRow, BorderLayout.NORTH)
            add(JBScrollPane(memoryArea).apply { border = BorderFactory.createLineBorder(borderColor) }, BorderLayout.CENTER)
        }
    }

    private fun buildFindingsTab(): JPanel {
        findingsList.cellRenderer = FindingCellRenderer(surface, muted)
        return JPanel(BorderLayout()).apply {
            border = JBUI.Borders.empty(10, 2)
            isOpaque = false
            add(JBScrollPane(findingsList).apply { border = BorderFactory.createLineBorder(borderColor) }, BorderLayout.CENTER)
        }
    }

    private fun buildTabs(): JBTabbedPane {
        return JBTabbedPane().apply {
            border = JBUI.Borders.emptyTop(8)
            addTab("Reality", buildRealityTab())
            addTab("Activity", buildActivityTab())
            addTab("Memory", buildMemoryTab())
            addTab("Findings", buildFindingsTab())
        }
    }

    private fun setBusy(value: Boolean, label: String = "") {
        progress.isVisible = value
        deepButton.isEnabled = !value
        scanButton.isEnabled = !value
        memoryButton.isEnabled = !value
        cursor = if (value) Cursor.getPredefinedCursor(Cursor.WAIT_CURSOR) else Cursor.getDefaultCursor()
        if (label.isNotBlank()) statusLabel.text = label
    }

    private fun runDeepAnalysis() {
        val projectPath = project.basePath ?: return
        setBusy(true, "Mapping deep architecture activity and model memory…")
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val bundle = CkbIntelligenceClient.bundle(projectPath)
                SwingUtilities.invokeLater {
                    applyBundle(bundle)
                    setBusy(false, "Living architecture memory hydrated from local CKB Core")
                }
            } catch (e: Exception) {
                SwingUtilities.invokeLater {
                    setBusy(false, "Deep intelligence unavailable: ${e.message}")
                }
            }
        }
    }

    private fun runBaseScan() {
        val projectPath = project.basePath ?: return
        setBusy(true, "Refreshing base architecture scan…")
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val report = CkbApiClient.scan(projectPath)
                SwingUtilities.invokeLater {
                    applyScan(report)
                    setBusy(false, "Base architecture refreshed • deep activity remains a separate operation")
                }
            } catch (e: Exception) {
                SwingUtilities.invokeLater { setBusy(false, "Base scan unavailable: ${e.message}") }
            }
        }
    }

    private fun queryMemory() {
        val projectPath = project.basePath ?: return
        val query = memoryQuery.text.trim()
        if (query.isBlank()) return
        setBusy(true, "Retrieving bounded architecture memory…")
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val response = CkbIntelligenceClient.memory(projectPath, query)
                val memory = response.getAsJsonObject("memory")
                val context = memory?.get("context")?.asString ?: "No architecture context returned."
                SwingUtilities.invokeLater {
                    memoryArea.text = context
                    memoryArea.caretPosition = 0
                    setBusy(false, "Architecture memory retrieved from the real local graph")
                }
            } catch (e: Exception) {
                SwingUtilities.invokeLater { setBusy(false, "Memory query unavailable: ${e.message}") }
            }
        }
    }

    private fun applyBundle(bundle: JsonObject) {
        val scan = bundle.getAsJsonObject("scan")
        val activity = bundle.getAsJsonObject("activity")
        val dna = bundle.getAsJsonObject("dna")
        val memory = bundle.getAsJsonObject("memory")

        symbolMetric.text = activity?.get("nodesAnalyzed")?.asInt?.toString() ?: scan?.get("nodes")?.asInt?.toString() ?: "—"
        relationMetric.text = activity?.get("edgesAnalyzed")?.asInt?.toString() ?: scan?.get("edges")?.asInt?.toString() ?: "—"
        runtimeMetric.text = activity?.get("runtimeCoveragePct")?.asDouble?.let { String.format("%.1f%%", it) } ?: "—"
        dnaMetric.text = dna?.get("overallHealth")?.asDouble?.let { String.format("%.1f%%", it) } ?: "—"
        sourceLabel.text = "LOCAL CKB CORE • ${bundle.get("evidencePolicy")?.asString ?: "static-runtime-predicted-separated"}"

        activityModel.clear()
        activity?.getAsJsonArray("hotspots")?.take(60)?.forEach { element ->
            val node = element.asJsonObject
            activityModel.addElement(ActivityItem(
                id = node.get("id")?.asString ?: "",
                name = node.get("name")?.asString ?: node.get("id")?.asString ?: "symbol",
                path = node.get("path")?.asString ?: "",
                role = node.get("role")?.asString ?: "architecture-symbol",
                activity = node.get("activityIndex")?.asDouble ?: 0.0,
                changeSensitivity = node.get("changeSensitivityIndex")?.asDouble ?: 0.0,
                runtimeObserved = node.get("runtimeObserved")?.asBoolean ?: false,
                fanIn = node.get("fanIn")?.asInt ?: 0,
                fanOut = node.get("fanOut")?.asInt ?: 0
            ))
        }

        val boundaryText = StringBuilder("ARCHITECTURE BOUNDARIES\n\n")
        activity?.getAsJsonArray("boundaries")?.take(30)?.forEach { element ->
            val boundary = element.asJsonObject
            boundaryText.append(boundary.get("id")?.asString ?: "boundary")
                .append("\n  symbols ").append(boundary.get("symbols")?.asInt ?: 0)
                .append(" • incoming ").append(boundary.get("incomingCrossBoundary")?.asInt ?: 0)
                .append(" • outgoing ").append(boundary.get("outgoingCrossBoundary")?.asInt ?: 0)
                .append(" • runtime ").append(boundary.get("runtimeObservedSymbols")?.asInt ?: 0)
                .append("\n\n")
        }
        boundaryArea.text = boundaryText.toString()
        boundaryArea.caretPosition = 0

        memoryArea.text = memory?.get("context")?.asString ?: "Run a memory query to retrieve bounded architecture context."
        memoryArea.caretPosition = 0
        applyFindings(scan)
    }

    private fun applyScan(report: ScanReport) {
        symbolMetric.text = report.nodes.toString()
        relationMetric.text = report.edges.toString()
        runtimeMetric.text = "—"
        dnaMetric.text = "—"
        sourceLabel.text = "BASE STATIC SCAN • runtime/deep memory not inferred"
        findingsModel.clear()
        val rank = mapOf("Critical" to 4, "Error" to 3, "Warning" to 2, "Info" to 1)
        report.drift.sortedByDescending { rank[it.severity] ?: 0 }.forEach { finding ->
            findingsModel.addElement("${finding.severity.uppercase()} • ${finding.kind} • ${finding.message}")
        }
    }

    private fun applyFindings(scan: JsonObject?) {
        findingsModel.clear()
        scan?.getAsJsonArray("drift")?.forEach { element ->
            val finding = element.asJsonObject
            val severity = finding.get("severity")?.asString ?: "Info"
            val kind = finding.get("kind")?.asString ?: "architecture"
            val message = finding.get("message")?.asString ?: "Architecture finding"
            findingsModel.addElement("${severity.uppercase()} • $kind • $message")
        }
    }

    private fun openActivitySource(item: ActivityItem?) {
        if (item == null || item.path.isBlank()) return
        val root = project.basePath ?: return
        val candidate = File(item.path).let { if (it.isAbsolute) it else File(root, item.path) }
        val virtualFile = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(candidate) ?: return
        FileEditorManager.getInstance(project).openFile(virtualFile, true)
    }

    private class ActivityCellRenderer(
        private val cyan: Color,
        private val green: Color,
        private val purple: Color,
        private val muted: Color,
        private val surface: Color
    ) : JPanel(BorderLayout()), javax.swing.ListCellRenderer<ActivityItem> {
        private val title = JBLabel()
        private val subtitle = JBLabel()
        private val score = JBLabel()

        init {
            border = JBUI.Borders.empty(7, 9)
            val text = JPanel().apply {
                layout = BoxLayout(this, BoxLayout.Y_AXIS)
                isOpaque = false
                add(title)
                add(subtitle)
            }
            add(text, BorderLayout.CENTER)
            add(score, BorderLayout.EAST)
        }

        override fun getListCellRendererComponent(
            list: JList<out ActivityItem>, value: ActivityItem, index: Int, isSelected: Boolean, cellHasFocus: Boolean
        ): Component {
            isOpaque = true
            background = if (isSelected) JBColor(Color(224, 244, 249), Color(18, 37, 49)) else surface
            title.text = "${value.name}  •  ${value.role}"
            title.foreground = if (value.runtimeObserved) green else cyan
            title.font = title.font.deriveFont(Font.BOLD, 11.5f)
            subtitle.text = "${value.path}  •  fan-in ${value.fanIn} / fan-out ${value.fanOut}"
            subtitle.foreground = muted
            subtitle.font = subtitle.font.deriveFont(9.5f)
            score.text = "A ${(value.activity * 100).toInt()}%   Δ ${(value.changeSensitivity * 100).toInt()}%"
            score.foreground = if (value.changeSensitivity >= 0.65) purple else muted
            score.font = score.font.deriveFont(Font.BOLD, 10f)
            return this
        }
    }

    private class FindingCellRenderer(private val surface: Color, private val muted: Color) : DefaultListCellRenderer() {
        override fun getListCellRendererComponent(
            list: JList<*>, value: Any?, index: Int, isSelected: Boolean, cellHasFocus: Boolean
        ): Component {
            super.getListCellRendererComponent(list, value, index, isSelected, cellHasFocus)
            border = JBUI.Borders.empty(7, 10)
            font = font.deriveFont(11f)
            if (!isSelected) {
                background = surface
                val text = (value ?: "").toString()
                foreground = when {
                    text.startsWith("CRITICAL") || text.startsWith("ERROR") -> JBColor(Color(174, 42, 65), Color(255, 97, 123))
                    text.startsWith("WARNING") -> JBColor(Color(160, 102, 24), Color(255, 189, 102))
                    else -> muted
                }
            }
            return this
        }
    }
}
