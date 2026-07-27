package dev.ckb.toolwindow

import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.content.ContentFactory
import com.intellij.util.ui.JBUI
import dev.ckb.api.CkbApiClient
import dev.ckb.api.DriftViolation
import dev.ckb.api.ScanReport
import java.awt.BorderLayout
import java.awt.Color
import java.awt.Component
import java.awt.Dimension
import javax.swing.*

class CkbToolWindowFactory : ToolWindowFactory {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = CkbToolWindowPanel(project)
        val content = ContentFactory.getInstance().createContent(panel, "", false)
        toolWindow.contentManager.addContent(content)
    }
}

class CkbToolWindowPanel(private val project: Project) : JPanel(BorderLayout()) {
    private val statusLabel = JBLabel("CKB — Click Scan to analyze your project")
    private val violationsModel = DefaultListModel<String>()
    private val violationsList = JList(violationsModel)
    private val scanButton = JButton("⟳ Scan Project")
    private val statsLabel = JBLabel("")

    init {
        border = JBUI.Borders.empty(8)

        // Top bar
        val topPanel = JPanel(BorderLayout())
        topPanel.add(statusLabel, BorderLayout.CENTER)
        topPanel.add(scanButton, BorderLayout.EAST)
        add(topPanel, BorderLayout.NORTH)

        // Stats
        statsLabel.font = statsLabel.font.deriveFont(11f)
        statsLabel.foreground = Color.GRAY
        add(statsLabel, BorderLayout.SOUTH)

        // Violations list
        violationsList.cellRenderer = ViolationCellRenderer()
        val scrollPane = JBScrollPane(violationsList)
        scrollPane.preferredSize = Dimension(400, 300)
        add(scrollPane, BorderLayout.CENTER)

        scanButton.addActionListener { doScan() }
    }

    private fun doScan() {
        scanButton.isEnabled = false
        statusLabel.text = "Scanning..."
        violationsModel.clear()

        Thread {
            try {
                val projectPath = project.basePath ?: return@Thread
                val report = CkbApiClient.scan(projectPath)
                SwingUtilities.invokeLater { updateUI(report) }
            } catch (e: Exception) {
                SwingUtilities.invokeLater {
                    statusLabel.text = "⚠ Error: ${e.message}"
                    scanButton.isEnabled = true
                }
            }
        }.start()
    }

    private fun updateUI(report: ScanReport) {
        val violations = report.drift.size
        val criticals = report.drift.count { it.severity == "Critical" || it.severity == "Error" }
        val icon = if (criticals > 0) "🔴" else if (violations > 0) "🟡" else "✅"
        statusLabel.text = "$icon Last scan: $violations violations"
        statsLabel.text = "  ${report.files_processed} files · ${report.nodes} nodes · ${report.edges} edges · ${report.patterns.size} patterns"

        violationsModel.clear()
        report.drift.sortedByDescending { it.severity }.forEach { v ->
            val severityIcon = when (v.severity) {
                "Critical" -> "🔴"
                "Error" -> "🟠"
                "Warning" -> "🟡"
                else -> "🔵"
            }
            violationsModel.addElement("$severityIcon [${v.kind}] ${v.message}")
        }
        scanButton.isEnabled = true
    }

    private class ViolationCellRenderer : DefaultListCellRenderer() {
        override fun getListCellRendererComponent(
            list: JList<*>, value: Any?, index: Int, isSelected: Boolean, cellHasFocus: Boolean
        ): Component {
            super.getListCellRendererComponent(list, value, index, isSelected, cellHasFocus)
            val text = value as? String ?: ""
            border = JBUI.Borders.empty(4, 8)
            font = font.deriveFont(12f)
            if (!isSelected) {
                background = if (index % 2 == 0) Color(30, 30, 40) else Color(25, 25, 35)
                foreground = when {
                    text.startsWith("🔴") -> Color(255, 100, 100)
                    text.startsWith("🟠") -> Color(255, 160, 80)
                    text.startsWith("🟡") -> Color(240, 200, 80)
                    else -> Color(150, 180, 255)
                }
            }
            return this
        }
    }
}
