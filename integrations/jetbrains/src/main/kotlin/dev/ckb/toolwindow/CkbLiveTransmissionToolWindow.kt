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
import com.intellij.ui.components.JBTextArea
import com.intellij.ui.content.ContentFactory
import com.intellij.util.ui.JBUI
import dev.ckb.api.CkbRuntimeRealityClient
import java.awt.BasicStroke
import java.awt.BorderLayout
import java.awt.Color
import java.awt.Dimension
import java.awt.FlowLayout
import java.awt.Font
import java.awt.Graphics
import java.awt.Graphics2D
import java.awt.RenderingHints
import java.io.File
import javax.swing.BorderFactory
import javax.swing.Box
import javax.swing.BoxLayout
import javax.swing.DefaultListModel
import javax.swing.JButton
import javax.swing.JPanel
import javax.swing.JProgressBar
import javax.swing.SwingUtilities
import javax.swing.Timer
import kotlin.math.max

class CkbLiveTransmissionToolWindowFactory : ToolWindowFactory {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = CkbLiveTransmissionPanel(project)
        val content = ContentFactory.getInstance().createContent(panel, "Live Reality V8", false)
        toolWindow.contentManager.addContent(content)
    }
}

data class RuntimeFlowStep(
    val traceId: String,
    val source: String,
    val target: String,
    val operation: String,
    val flowType: String,
    val durationMs: Double,
    val error: Boolean
) {
    override fun toString(): String = "${flowType.uppercase()}  ${source.substringAfterLast("/").take(30)} → ${target.substringAfterLast("/").take(30)}"
}

private fun normalizedFlow(step: JsonObject): String {
    val explicit = step.get("flowType")?.asString.orEmpty().lowercase()
    val context = listOf(
        explicit,
        step.get("operation")?.asString.orEmpty(),
        step.get("protocol")?.asString.orEmpty(),
        step.get("dbSystem")?.asString.orEmpty(),
        step.get("messagingSystem")?.asString.orEmpty()
    ).joinToString(" ").lowercase()
    return when {
        Regex("websocket|\\bws\\b|\\bwss\\b").containsMatchIn(context) -> "websocket"
        Regex("redis|cache").containsMatchIn(context) -> "cache"
        Regex("postgres|mysql|sqlite|mongo|prisma|database|\\bsql\\b").containsMatchIn(context) -> "database"
        Regex("queue|kafka|rabbit|bull|sqs|pubsub|message").containsMatchIn(context) -> "queue"
        Regex("event").containsMatchIn(context) -> "event"
        Regex("http|rpc|fetch|request|response").containsMatchIn(context) -> "http"
        else -> "function"
    }
}

class ExactRuntimeFlowCanvas : JPanel() {
    private var steps: List<RuntimeFlowStep> = emptyList()
    private var phase = 0.0
    private val animation = Timer(70) {
        if (steps.isNotEmpty()) {
            phase = (phase + 0.035) % 1.0
            repaint()
        }
    }

    init {
        preferredSize = Dimension(520, 170)
        minimumSize = Dimension(320, 130)
        isOpaque = false
        animation.start()
    }

    fun setTrace(next: List<RuntimeFlowStep>) {
        steps = next.take(10)
        repaint()
    }

    private fun colorFor(type: String, error: Boolean): Color {
        if (error) return Color(255, 117, 142)
        return when (type) {
            "http" -> Color(73, 232, 255)
            "database" -> Color(251, 191, 36)
            "cache" -> Color(96, 165, 250)
            "queue" -> Color(192, 132, 252)
            "event" -> Color(167, 139, 250)
            "websocket" -> Color(244, 114, 182)
            else -> Color(88, 239, 169)
        }
    }

    override fun paintComponent(graphics: Graphics) {
        super.paintComponent(graphics)
        val g = graphics.create() as Graphics2D
        try {
            g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
            val w = width.coerceAtLeast(320)
            val h = height.coerceAtLeast(120)
            g.color = JBColor(Color(245, 249, 252), Color(4, 9, 18))
            g.fillRoundRect(0, 0, w, h, 18, 18)
            if (steps.isEmpty()) {
                g.color = JBColor(Color(90, 105, 120), Color(126, 145, 171))
                g.font = font.deriveFont(Font.PLAIN, 12f)
                g.drawString("Exact observed parent/child spans will animate here when runtime telemetry arrives.", 18, h / 2)
                return
            }
            val nodes = mutableListOf<String>()
            nodes.add(steps.first().source)
            steps.forEach { nodes.add(it.target) }
            val count = nodes.size.coerceAtMost(11)
            val left = 24
            val right = w - 24
            val y = h / 2
            val spacing = if (count <= 1) 1 else (right - left).toDouble() / (count - 1)
            for (i in 0 until count - 1) {
                val x1 = (left + i * spacing).toInt()
                val x2 = (left + (i + 1) * spacing).toInt()
                val step = steps[i]
                val color = colorFor(step.flowType, step.error)
                g.color = Color(color.red, color.green, color.blue, 150)
                g.stroke = BasicStroke(if (step.error) 3f else 2f)
                g.drawLine(x1, y, x2, y)
                val pulseX = (x1 + (x2 - x1) * phase).toInt()
                g.color = color
                g.fillOval(pulseX - 4, y - 4, 8, 8)
            }
            for (i in 0 until count) {
                val x = (left + i * spacing).toInt()
                val color = if (i == 0) Color(73, 232, 255) else colorFor(steps[i - 1].flowType, steps[i - 1].error)
                g.color = Color(color.red, color.green, color.blue, 42)
                g.fillOval(x - 12, y - 12, 24, 24)
                g.color = color
                g.fillOval(x - 5, y - 5, 10, 10)
            }
            g.color = JBColor(Color(60, 75, 92), Color(145, 162, 184))
            g.font = font.deriveFont(Font.BOLD, 9f)
            g.drawString("EXACT OBSERVED EXECUTION • moving pulses are never generated from static dependencies", 14, 18)
        } finally {
            g.dispose()
        }
    }

    override fun removeNotify() {
        animation.stop()
        super.removeNotify()
    }

    override fun addNotify() {
        super.addNotify()
        if (!animation.isRunning) animation.start()
    }
}

class CkbLiveTransmissionPanel(private val project: Project) : JPanel(BorderLayout()) {
    private val cyan = JBColor(Color(0, 128, 150), Color(73, 232, 255))
    private val green = JBColor(Color(18, 140, 90), Color(88, 239, 169))
    private val violet = JBColor(Color(118, 72, 180), Color(201, 156, 255))
    private val red = JBColor(Color(190, 55, 75), Color(255, 117, 142))
    private val muted = JBColor(Color(85, 100, 120), Color(126, 145, 171))
    private val borderColor = JBColor(Color(205, 217, 229), Color(31, 50, 68))
    private val surface = JBColor(Color(247, 250, 253), Color(7, 13, 24))
    private val status = JBLabel("Runtime Reality waiting for connection")
    private val progress = JProgressBar().apply { isIndeterminate = true; isVisible = false }
    private val refreshButton = JButton("↻ Refresh Live")
    private val previousButton = JButton("← Prev")
    private val nextButton = JButton("Next →")
    private val model = DefaultListModel<RuntimeFlowStep>()
    private val list = JBList(model)
    private val detail = JBTextArea()
    private val canvas = ExactRuntimeFlowCanvas()
    private val filters = linkedMapOf<String, JButton>()
    private var allSteps: List<RuntimeFlowStep> = emptyList()
    private var traceGroups: Map<String, List<RuntimeFlowStep>> = emptyMap()
    private var activeTraceId = ""
    private var selectedIndex = 0
    private var filter = "all"
    private var refreshing = false
    private var replaySafe = false
    private val pollTimer = Timer(2500) { if (isShowing) refresh(true) }

    init {
        background = surface
        border = JBUI.Borders.empty(10)
        add(buildHeader(), BorderLayout.NORTH)
        add(buildBody(), BorderLayout.CENTER)
        refreshButton.addActionListener { refresh(false) }
        previousButton.addActionListener { moveStep(-1) }
        nextButton.addActionListener { moveStep(1) }
        list.addListSelectionListener { if (!it.valueIsAdjusting) selectListStep() }
        list.addMouseListener(object : java.awt.event.MouseAdapter() {
            override fun mouseClicked(e: java.awt.event.MouseEvent) {
                if (e.clickCount == 2) openSelectedSource()
            }
        })
        pollTimer.start()
        refresh(true)
    }

    private fun buildHeader(): JPanel {
        val copy = JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS)
            isOpaque = false
            add(JBLabel("●  CKB LIVE TRANSMISSION FIELD V8").apply { foreground = green; font = font.deriveFont(Font.BOLD, 11f) })
            add(Box.createVerticalStrut(4))
            add(JBLabel("Observed software execution inside the IDE").apply { font = font.deriveFont(Font.BOLD, 17f) })
            add(Box.createVerticalStrut(3))
            add(status.apply { foreground = muted })
            add(JBLabel("HTTP • DB • CACHE • QUEUE • EVENT • WEBSOCKET • FUNCTION").apply { foreground = cyan; font = font.deriveFont(Font.BOLD, 9f) })
        }
        return JPanel(BorderLayout()).apply {
            isOpaque = false
            border = BorderFactory.createCompoundBorder(BorderFactory.createLineBorder(borderColor), JBUI.Borders.empty(12))
            add(copy, BorderLayout.CENTER)
            add(refreshButton, BorderLayout.EAST)
            add(progress, BorderLayout.SOUTH)
        }
    }

    private fun buildBody(): JPanel {
        list.fixedCellHeight = 42
        detail.apply {
            isEditable = false
            lineWrap = true
            wrapStyleWord = true
            background = surface
            foreground = muted
            border = JBUI.Borders.empty(8)
            text = "Connect a running deployment/local service to CKB Live Reality. Static relationships remain static until execution is actually observed."
        }
        val filterRail = JPanel(FlowLayout(FlowLayout.LEFT, 5, 2)).apply {
            isOpaque = false
            listOf("all", "http", "database", "cache", "queue", "event", "websocket", "function").forEach { type ->
                val button = JButton(type.uppercase())
                filters[type] = button
                button.addActionListener { filter = type; render() }
                add(button)
            }
        }
        val sequencerControls = JPanel(FlowLayout(FlowLayout.RIGHT, 5, 2)).apply {
            isOpaque = false
            add(previousButton)
            add(nextButton)
        }
        val split = JPanel(java.awt.GridLayout(1, 2, 8, 0)).apply {
            isOpaque = false
            add(JBScrollPane(list).apply { border = BorderFactory.createLineBorder(borderColor) })
            add(JBScrollPane(detail).apply { border = BorderFactory.createLineBorder(borderColor) })
        }
        return JPanel(BorderLayout(0, 8)).apply {
            isOpaque = false
            border = JBUI.Borders.emptyTop(8)
            add(JPanel(BorderLayout()).apply { isOpaque = false; add(filterRail, BorderLayout.CENTER); add(sequencerControls, BorderLayout.EAST) }, BorderLayout.NORTH)
            add(JPanel(BorderLayout(0, 8)).apply { isOpaque = false; add(canvas, BorderLayout.NORTH); add(split, BorderLayout.CENTER) }, BorderLayout.CENTER)
        }
    }

    private fun refresh(quiet: Boolean) {
        if (refreshing) return
        refreshing = true
        if (!quiet) {
            progress.isVisible = true
            status.text = "Reading exact runtime evidence…"
        }
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val (traceData, runtimeData) = CkbRuntimeRealityClient.snapshot()
                val tracesObject = traceData.getAsJsonObject("traces")
                val safe = traceData.get("replaySafe")?.asBoolean == true && traceData.get("traceSemantics")?.asString == "exact-observed-span-instances"
                val groups = linkedMapOf<String, List<RuntimeFlowStep>>()
                tracesObject?.entrySet()?.forEach { (traceId, value) ->
                    val rows = mutableListOf<RuntimeFlowStep>()
                    if (value.isJsonArray) value.asJsonArray.forEach { element ->
                        val step = element.asJsonObject
                        rows.add(RuntimeFlowStep(
                            traceId = traceId,
                            source = step.get("source")?.asString ?: "unknown",
                            target = step.get("target")?.asString ?: "unknown",
                            operation = step.get("operation")?.asString ?: step.get("name")?.asString ?: "runtime transition",
                            flowType = normalizedFlow(step),
                            durationMs = step.get("durationMs")?.asDouble ?: 0.0,
                            error = step.get("error")?.asBoolean ?: false
                        ))
                    }
                    if (rows.isNotEmpty()) groups[traceId] = rows
                }
                val runtimeNodes = runtimeData.getAsJsonArray("nodes")?.size() ?: runtimeData.get("runtimeNodes")?.asInt ?: 0
                SwingUtilities.invokeLater {
                    replaySafe = safe
                    traceGroups = groups
                    allSteps = groups.values.flatten()
                    if (activeTraceId.isBlank() || !groups.containsKey(activeTraceId)) activeTraceId = groups.keys.firstOrNull().orEmpty()
                    selectedIndex = selectedIndex.coerceIn(0, max(0, (groups[activeTraceId]?.size ?: 1) - 1))
                    status.text = when {
                        groups.isNotEmpty() && safe -> "LIVE • ${groups.size} exact traces • $runtimeNodes runtime nodes • ${allSteps.size} observed transitions"
                        runtimeNodes > 0 -> "Runtime observed • exact parent/child retrace is not available yet"
                        else -> "Engine online • waiting for observed application execution"
                    }
                    progress.isVisible = false
                    refreshing = false
                    render()
                }
            } catch (e: Exception) {
                SwingUtilities.invokeLater {
                    status.text = "Runtime server unavailable • static IDE intelligence remains usable"
                    progress.isVisible = false
                    refreshing = false
                    if (!quiet) detail.text = "Live Reality connection failed: ${e.message}\n\nConfigure the CKB Reality server URL, then attach runtime telemetry. CKB will not synthesize missing execution."
                }
            }
        }
    }

    private fun render() {
        model.clear()
        val visible = allSteps.filter { filter == "all" || it.flowType == filter }
        visible.takeLast(120).forEach { model.addElement(it) }
        filters.forEach { (key, button) -> button.foreground = if (key == filter) cyan else JBColor.foreground() }
        val trace = traceGroups[activeTraceId].orEmpty()
        canvas.setTrace(trace)
        previousButton.isEnabled = replaySafe && trace.isNotEmpty() && selectedIndex > 0
        nextButton.isEnabled = replaySafe && trace.isNotEmpty() && selectedIndex < trace.lastIndex
        if (trace.isNotEmpty()) showTraceStep(trace[selectedIndex])
        else detail.text = "No exact observed transitions match the current runtime state.\n\nThe animated canvas activates only from exact observed parent/child spans; static architecture is never rendered as live execution."
    }

    private fun moveStep(delta: Int) {
        val trace = traceGroups[activeTraceId].orEmpty()
        if (trace.isEmpty()) return
        selectedIndex = (selectedIndex + delta).coerceIn(0, trace.lastIndex)
        showTraceStep(trace[selectedIndex])
        previousButton.isEnabled = selectedIndex > 0
        nextButton.isEnabled = selectedIndex < trace.lastIndex
    }

    private fun selectListStep() {
        val selected = list.selectedValue ?: return
        activeTraceId = selected.traceId
        val trace = traceGroups[activeTraceId].orEmpty()
        selectedIndex = trace.indexOfFirst { it == selected }.coerceAtLeast(0)
        canvas.setTrace(trace)
        showTraceStep(selected)
    }

    private fun showTraceStep(step: RuntimeFlowStep) {
        detail.foreground = if (step.error) red else muted
        detail.text = buildString {
            append("${step.flowType.uppercase()} • EXACT OBSERVED\n\n")
            append("${step.source}\n→ ${step.target}\n\n")
            append("Operation: ${step.operation}\n")
            append("Duration: ${String.format("%.2f", step.durationMs)} ms\n")
            append("Error observed: ${if (step.error) "yes" else "no"}\n")
            append("Trace: ${step.traceId}\n\n")
            append("Double-click this row to open the target source when its CKB identity maps to a local file.")
        }
    }

    private fun openSelectedSource() {
        val step = list.selectedValue ?: traceGroups[activeTraceId]?.getOrNull(selectedIndex) ?: return
        val root = project.basePath ?: return
        val raw = step.target.substringBefore("::")
        if (raw.isBlank() || raw == "unknown") return
        val file = if (File(raw).isAbsolute) File(raw) else File(root, raw)
        val virtualFile = LocalFileSystem.getInstance().findFileByIoFile(file) ?: return
        FileEditorManager.getInstance(project).openFile(virtualFile, true)
    }

    override fun removeNotify() {
        pollTimer.stop()
        super.removeNotify()
    }

    override fun addNotify() {
        super.addNotify()
        if (!pollTimer.isRunning) pollTimer.start()
    }
}
