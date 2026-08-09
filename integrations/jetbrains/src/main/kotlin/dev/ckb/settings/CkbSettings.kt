package dev.ckb.settings

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.options.Configurable
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

@State(name = "CkbSettings", storages = [Storage("CkbSettings.xml")])
class CkbSettings : PersistentStateComponent<CkbSettings.State> {

    data class State(
        var serverUrl: String = "http://localhost:3000",
        var intelligenceBinary: String = "ckb-intelligence",
        var autoScanOnOpen: Boolean = true,
        var deepAnalysisOnOpen: Boolean = true,
        var showInlineAnnotations: Boolean = true
    )

    private var myState = State()

    override fun getState(): State = myState
    override fun loadState(state: State) { myState = state }

    var serverUrl: String
        get() = myState.serverUrl
        set(value) { myState = myState.copy(serverUrl = value) }

    var intelligenceBinary: String
        get() = myState.intelligenceBinary
        set(value) { myState = myState.copy(intelligenceBinary = value) }

    var autoScanOnOpen: Boolean
        get() = myState.autoScanOnOpen
        set(value) { myState = myState.copy(autoScanOnOpen = value) }

    var deepAnalysisOnOpen: Boolean
        get() = myState.deepAnalysisOnOpen
        set(value) { myState = myState.copy(deepAnalysisOnOpen = value) }

    var showInlineAnnotations: Boolean
        get() = myState.showInlineAnnotations
        set(value) { myState = myState.copy(showInlineAnnotations = value) }

    companion object {
        val instance: CkbSettings
            get() = ApplicationManager.getApplication().getService(CkbSettings::class.java)
    }
}

class CkbSettingsConfigurable : Configurable {
    private var serverUrlField: JBTextField? = null
    private var intelligenceBinaryField: JBTextField? = null
    private var autoScanBox: JBCheckBox? = null
    private var deepAnalysisBox: JBCheckBox? = null

    override fun getDisplayName(): String = "CKB Living Architecture"

    override fun createComponent(): JComponent {
        val settings = CkbSettings.instance
        serverUrlField = JBTextField(settings.serverUrl)
        intelligenceBinaryField = JBTextField(settings.intelligenceBinary)
        autoScanBox = JBCheckBox("Refresh base architecture when a project opens", settings.autoScanOnOpen)
        deepAnalysisBox = JBCheckBox("Hydrate deep activity + architecture memory on project open", settings.deepAnalysisOnOpen)
        return FormBuilder.createFormBuilder()
            .addLabeledComponent("CKB server fallback URL:", serverUrlField!!, 1, false)
            .addLabeledComponent("Local intelligence binary:", intelligenceBinaryField!!, 1, false)
            .addComponent(autoScanBox!!)
            .addComponent(deepAnalysisBox!!)
            .addSeparator()
            .addComponentFillVertically(JPanel(), 0)
            .panel
    }

    override fun isModified(): Boolean {
        val settings = CkbSettings.instance
        return serverUrlField?.text != settings.serverUrl
            || intelligenceBinaryField?.text != settings.intelligenceBinary
            || autoScanBox?.isSelected != settings.autoScanOnOpen
            || deepAnalysisBox?.isSelected != settings.deepAnalysisOnOpen
    }

    override fun apply() {
        val settings = CkbSettings.instance
        settings.serverUrl = serverUrlField?.text?.trim().orEmpty().ifBlank { "http://localhost:3000" }
        settings.intelligenceBinary = intelligenceBinaryField?.text?.trim().orEmpty().ifBlank { "ckb-intelligence" }
        settings.autoScanOnOpen = autoScanBox?.isSelected ?: true
        settings.deepAnalysisOnOpen = deepAnalysisBox?.isSelected ?: true
    }

    override fun reset() {
        val settings = CkbSettings.instance
        serverUrlField?.text = settings.serverUrl
        intelligenceBinaryField?.text = settings.intelligenceBinary
        autoScanBox?.isSelected = settings.autoScanOnOpen
        deepAnalysisBox?.isSelected = settings.deepAnalysisOnOpen
    }
}
