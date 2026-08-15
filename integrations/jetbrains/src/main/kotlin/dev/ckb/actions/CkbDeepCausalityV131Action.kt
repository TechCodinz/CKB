package dev.ckb.actions

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.Task
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File
import java.util.concurrent.TimeUnit

private data class CausalityOp(val id: String, val label: String, val detail: String)

private val CAUSALITY_OPS = listOf(
    CausalityOp("data-flow", "Data Flow", "Interprocedural value/data path"),
    CausalityOp("taint", "Taint + Trust Boundaries", "Unsanitized source → sink paths"),
    CausalityOp("reachable", "Path-Sensitive Reachability", "Reachability under recorded conditions"),
    CausalityOp("constraints", "Symbolic Constraints", "Equality, inequality and numeric ranges"),
    CausalityOp("concurrency", "Concurrency Hazards", "Multi-writers, locks and deadlock cycles"),
    CausalityOp("schema-impact", "Schema + Migration Impact", "Database/schema blast radius"),
    CausalityOp("infra-impact", "Infrastructure Impact", "IaC/deployment blast radius"),
    CausalityOp("config-impact", "Config + Feature Flag Causality", "Configuration dependents"),
    CausalityOp("distributed-flow", "Distributed/Event Flow", "Queue, topic, event, job and service flow"),
    CausalityOp("contract-diff", "API/Schema Evolution", "Backward compatibility classification"),
    CausalityOp("tests", "Behavioral Test Selection", "Tests connected to changed entities"),
    CausalityOp("policy", "Architecture Invariants", "Executable architecture policy rules"),
    CausalityOp("drift-forecast", "Drift Forecast", "Bounded structural trend forecast (PREDICTED)"),
    CausalityOp("simulate", "Proposed Change Simulation", "Pre-edit impact, always PREDICTED"),
    CausalityOp("hotspots", "Runtime Resource Intelligence", "Observed CPU/memory/latency/error hotspots"),
    CausalityOp("failure-propagation", "Failure Propagation", "Cascading dependency impact"),
    CausalityOp("temporal-diff", "Temporal Architecture", "Architecture evidence diff across snapshots"),
    CausalityOp("cross-repo", "Cross-Repository Architecture", "Causal path across repository boundaries"),
    CausalityOp("ownership", "Ownership + Bus Factor", "Socio-technical ownership risk"),
    CausalityOp("quality", "Architecture Quality Metrics", "Evidence-derived coupling/cycles/instability"),
)

private fun causalityBinary(): String =
    System.getenv("CKB_CAUSALITY_BINARY")?.trim()?.takeIf { it.isNotEmpty() } ?: "ckb-causality"

private fun prompt(project: com.intellij.openapi.project.Project, title: String, message: String, initial: String = ""): String? =
    Messages.showInputDialog(project, message, title, null, initial, null)?.trim()?.takeIf { it.isNotEmpty() }

private fun resolveFile(root: File, value: String): String =
    File(value).let { if (it.isAbsolute) it else File(root, value) }.absolutePath

private fun queryArgs(project: com.intellij.openapi.project.Project, root: File, op: CausalityOp): List<String>? {
    return when (op.id) {
        "data-flow", "distributed-flow", "cross-repo" -> {
            val source = prompt(project, "CKB ${op.label}", "Source causal entity id") ?: return null
            val sink = prompt(project, "CKB ${op.label}", "Target/sink causal entity id") ?: return null
            listOf(source, sink)
        }
        "taint" -> {
            val sources = prompt(project, "CKB Taint", "Comma-separated source entity ids") ?: return null
            val sinks = prompt(project, "CKB Taint", "Comma-separated sink entity ids") ?: return null
            listOf("--sources=$sources", "--sinks=$sinks")
        }
        "reachable" -> {
            val source = prompt(project, "CKB Reachability", "Source causal entity id") ?: return null
            val sink = prompt(project, "CKB Reachability", "Target causal entity id") ?: return null
            val conditions = prompt(project, "CKB Reachability", "Optional comma-separated exact conditions", "authenticated,role==admin")
            buildList { add(source); add(sink); if (!conditions.isNullOrBlank()) add("--conditions=$conditions") }
        }
        "constraints" -> {
            val constraints = prompt(project, "CKB Symbolic Constraints", "Comma-separated constraints", "age>=18,age<65,active==true") ?: return null
            listOf("--constraints=$constraints")
        }
        "schema-impact", "infra-impact", "config-impact", "failure-propagation" -> {
            val entity = prompt(project, "CKB ${op.label}", "Causal entity id") ?: return null
            listOf(entity)
        }
        "contract-diff" -> {
            val before = prompt(project, "CKB Contract Diff", "Before ApiContract JSON file") ?: return null
            val after = prompt(project, "CKB Contract Diff", "After ApiContract JSON file") ?: return null
            listOf(resolveFile(root, before), resolveFile(root, after))
        }
        "tests" -> {
            val changed = prompt(project, "CKB Tests for Change", "Comma-separated changed entity ids") ?: return null
            listOf("--changed=$changed")
        }
        "policy" -> {
            val rules = prompt(project, "CKB Architecture Policy", "ArchitectureRule[] JSON file") ?: return null
            listOf(resolveFile(root, rules))
        }
        "drift-forecast" -> {
            val counts = prompt(project, "CKB Drift Forecast", "Historical relation counts, comma-separated", "120,128,137,149") ?: return null
            listOf("--edge-counts=$counts")
        }
        "simulate" -> {
            val operations = prompt(project, "CKB Change Simulation", "ChangeOperation[] JSON file") ?: return null
            listOf(resolveFile(root, operations))
        }
        "temporal-diff" -> {
            val older = prompt(project, "CKB Temporal Architecture", "Older DeepCausalityEngine bundle") ?: return null
            listOf(resolveFile(root, older))
        }
        "concurrency", "hotspots", "ownership", "quality" -> emptyList()
        else -> emptyList()
    }
}

private fun execute(root: File, args: List<String>, timeoutSeconds: Long = 180): String {
    val command = mutableListOf(causalityBinary()).apply { addAll(args) }
    val process = ProcessBuilder(command)
        .directory(root)
        .redirectErrorStream(true)
        .start()
    val completed = process.waitFor(timeoutSeconds, TimeUnit.SECONDS)
    if (!completed) {
        process.destroyForcibly()
        error("CKB causality command timed out after ${timeoutSeconds}s")
    }
    val output = process.inputStream.bufferedReader().use { it.readText() }
    if (process.exitValue() != 0) error(output.ifBlank { "CKB causality command failed (${process.exitValue()})" })
    return output.trim()
}

private fun openCausalityJson(project: com.intellij.openapi.project.Project, title: String, json: String) {
    val temp = kotlin.io.path.createTempFile("ckb-causality-", ".json").toFile()
    temp.writeText(json.ifBlank { "{}" })
    temp.deleteOnExit()
    val file = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(temp)
    if (file != null) FileEditorManager.getInstance(project).openFile(file, true)
    else Messages.showInfoMessage(project, json.take(12_000), title)
}

class DeepCausalityV131Action : AnAction("Deep Software Causality") {
    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val rootPath = project.basePath ?: run {
            Messages.showWarningDialog(project, "Open a project first.", "CKB V13.1")
            return
        }
        val root = File(rootPath)
        val labels = CAUSALITY_OPS.map { "${it.label} — ${it.detail}" }.toTypedArray()
        val index = Messages.showChooseDialog(
            project,
            "Choose an evidence-backed software causality operation. Runtime claims require observed runtime evidence; simulations/forecasts remain PREDICTED.",
            "CKB V13.1 • Deep Software Causality",
            null,
            labels,
            labels.first(),
        )
        if (index !in CAUSALITY_OPS.indices) return
        val op = CAUSALITY_OPS[index]
        val extraArgs = queryArgs(project, root, op) ?: return

        ProgressManager.getInstance().run(object : Task.Backgroundable(project, "CKB V13.1 • ${op.label}", true) {
            override fun run(indicator: ProgressIndicator) {
                indicator.isIndeterminate = true
                try {
                    execute(root, listOf("build", root.absolutePath, "--output", ".ckb/deep-causality.json"))
                    val bundle = File(root, ".ckb/deep-causality.json").absolutePath
                    val result = execute(root, listOf("--bundle", bundle, op.id) + extraArgs)
                    ApplicationManager.getApplication().invokeLater { openCausalityJson(project, "CKB ${op.label}", result) }
                } catch (error: Exception) {
                    ApplicationManager.getApplication().invokeLater {
                        val hint = if ((error.message ?: "").contains("Cannot run program"))
                            "\n\nBuild/install ckb-causality or set CKB_CAUSALITY_BINARY to its executable path."
                        else ""
                        Messages.showErrorDialog(project, (error.message ?: error.toString()) + hint, "CKB V13.1 Deep Causality")
                    }
                }
            }
        })
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.project?.basePath != null
    }
}
