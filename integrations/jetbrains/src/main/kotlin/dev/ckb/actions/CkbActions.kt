package dev.ckb.actions

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.Task
import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.ToolWindowManager
import dev.ckb.api.CkbApiClient
import dev.ckb.api.ScanReport

/** Runs a full CKB scan on the project. Shows progress and populates the CKB tool window. */
class ScanProjectAction : AnAction("Scan Project") {

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val projectPath = project.basePath ?: return

        ProgressManager.getInstance().run(object : Task.Backgroundable(project, "CKB: Scanning project...", true) {
            var report: ScanReport? = null
            var error: String? = null

            override fun run(indicator: ProgressIndicator) {
                indicator.isIndeterminate = true
                indicator.text = "Connecting to CKB server..."
                if (!CkbApiClient.health()) {
                    error = "CKB server is not running. Start it with: ckb serve"
                    return
                }
                indicator.text = "Scanning codebase..."
                report = CkbApiClient.scan(projectPath)
            }

            override fun onSuccess() {
                if (error != null) {
                    com.intellij.openapi.ui.Messages.showErrorDialog(project, error, "CKB Error")
                    return
                }
                val r = report ?: return
                val violations = r.drift.size
                val severity = if (r.drift.any { it.severity == "Critical" }) "🔴" else if (violations > 0) "🟡" else "✅"

                com.intellij.openapi.ui.Messages.showInfoMessage(
                    project,
                    "$severity Scan complete!\n\n" +
                    "📁 Files: ${r.files_processed}\n" +
                    "🏗️ Nodes: ${r.nodes}, Edges: ${r.edges}\n" +
                    "🔍 Patterns found: ${r.patterns.size}\n" +
                    "⚠️ Violations: $violations",
                    "CKB Scan Complete"
                )

                // Refresh tool window
                ApplicationManager.getApplication().invokeLater {
                    ToolWindowManager.getInstance(project).getToolWindow("CKB")?.activate(null)
                }
            }

            override fun onThrowable(error: Throwable) {
                com.intellij.openapi.ui.Messages.showErrorDialog(
                    project, "Scan failed: ${error.message}", "CKB Error"
                )
            }
        })
    }
}

/** Checks architecture without opening a full scan. Shows pass/fail with violation count. */
class CheckArchitectureAction : AnAction("Check Architecture") {

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return

        ProgressManager.getInstance().run(object : Task.Backgroundable(project, "CKB: Checking architecture...", false) {
            var report: ScanReport? = null

            override fun run(indicator: ProgressIndicator) {
                indicator.isIndeterminate = true
                report = CkbApiClient.getReport()
            }

            override fun onSuccess() {
                val r = report ?: return
                val criticals = r.drift.count { it.severity == "Critical" || it.severity == "Error" }
                val warnings = r.drift.count { it.severity == "Warning" }

                if (r.drift.isEmpty()) {
                    com.intellij.openapi.ui.Messages.showInfoMessage(
                        project,
                        "✅ Architecture is clean!\n\nNo violations found in last scan.",
                        "CKB: Architecture Check"
                    )
                } else {
                    val msg = buildString {
                        appendLine("Found ${r.drift.size} violations:\n")
                        if (criticals > 0) appendLine("🔴 Critical/Errors: $criticals")
                        if (warnings > 0) appendLine("🟡 Warnings: $warnings")
                        appendLine()
                        r.drift.take(5).forEach { v ->
                            appendLine("• [${v.severity}] ${v.message}")
                            v.suggested_fix?.let { appendLine("  💡 $it") }
                        }
                        if (r.drift.size > 5) appendLine("...and ${r.drift.size - 5} more. Open the CKB panel to see all.")
                    }
                    com.intellij.openapi.ui.Messages.showWarningDialog(project, msg, "CKB: Architecture Violations")
                }
            }
        })
    }
}

/** Analyzes impact of a change at the current cursor position. */
class AnalyzeImpactAction : AnAction("Analyze Impact at Cursor") {

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        val projectPath = project.basePath ?: return

        val relativePath = file.path.removePrefix(projectPath).trimStart('/', '\\')
        val line = editor.caretModel.primaryCaret.logicalPosition.line + 1

        ProgressManager.getInstance().run(object : Task.Backgroundable(project, "CKB: Analyzing impact...", false) {
            var impact: dev.ckb.api.ImpactAnalysis? = null
            var error: String? = null

            override fun run(indicator: ProgressIndicator) {
                indicator.isIndeterminate = true
                indicator.text = "Calculating impact for $relativePath:$line..."
                impact = CkbApiClient.analyzeImpact(projectPath, relativePath, line)
            }

            override fun onSuccess() {
                val imp = impact ?: return
                val riskPct = (imp.risk_score * 100).toInt()
                val riskIcon = when {
                    riskPct >= 70 -> "🔴 HIGH"
                    riskPct >= 40 -> "🟡 MEDIUM"
                    else -> "🟢 LOW"
                }

                val msg = buildString {
                    appendLine("📍 $relativePath:$line")
                    appendLine("Risk: $riskIcon ($riskPct%)")
                    appendLine("Effort: ${imp.estimated_effort}")
                    appendLine()

                    if (imp.directly_affected.isNotEmpty()) {
                        appendLine("Directly affected (${imp.directly_affected.size}):")
                        imp.directly_affected.take(8).forEach { appendLine("  • $it") }
                        if (imp.directly_affected.size > 8)
                            appendLine("  ...and ${imp.directly_affected.size - 8} more")
                        appendLine()
                    }

                    if (imp.transitively_affected.isNotEmpty()) {
                        appendLine("Transitively affected (${imp.transitively_affected.size}):")
                        imp.transitively_affected.take(5).forEach { appendLine("  • $it") }
                        if (imp.transitively_affected.size > 5)
                            appendLine("  ...and ${imp.transitively_affected.size - 5} more")
                    }
                }

                if (imp.risk_score >= 0.7) {
                    com.intellij.openapi.ui.Messages.showWarningDialog(project, msg, "CKB Impact Analysis")
                } else {
                    com.intellij.openapi.ui.Messages.showInfoMessage(project, msg, "CKB Impact Analysis")
                }
            }

            override fun onThrowable(error: Throwable) {
                com.intellij.openapi.ui.Messages.showErrorDialog(
                    project,
                    "Impact analysis failed: ${error.message}\n\nMake sure you've scanned the project first (Tools → CKB → Scan Project).",
                    "CKB Error"
                )
            }
        })
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.project != null && e.getData(CommonDataKeys.EDITOR) != null
    }
}

/** Opens the CKB dashboard in the default browser */
class ShowGraphAction : AnAction("Show Dependency Graph") {
    override fun actionPerformed(e: AnActionEvent) {
        val settings = dev.ckb.settings.CkbSettings.instance
        val url = settings.serverUrl.replace("localhost:3000", "localhost:3001")
        com.intellij.ide.BrowserUtil.browse("$url/graph")
    }
}
