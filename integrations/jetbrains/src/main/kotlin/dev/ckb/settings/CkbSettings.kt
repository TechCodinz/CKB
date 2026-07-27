package dev.ckb.settings

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.options.Configurable
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

@State(name = "CkbSettings", storages = [Storage("CkbSettings.xml")])
class CkbSettings : PersistentStateComponent<CkbSettings.State> {

    data class State(
        var serverUrl: String = "http://localhost:3000",
        var autoScanOnOpen: Boolean = true,
        var showInlineAnnotations: Boolean = true
    )

    private var myState = State()

    override fun getState(): State = myState
    override fun loadState(state: State) { myState = state }

    var serverUrl: String
        get() = myState.serverUrl
        set(value) { myState = myState.copy(serverUrl = value) }

    var autoScanOnOpen: Boolean
        get() = myState.autoScanOnOpen
        set(value) { myState = myState.copy(autoScanOnOpen = value) }

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

    override fun getDisplayName(): String = "CKB Settings"

    override fun createComponent(): JComponent {
        serverUrlField = JBTextField(CkbSettings.instance.serverUrl)
        return FormBuilder.createFormBuilder()
            .addLabeledComponent("CKB Server URL:", serverUrlField!!, 1, false)
            .addComponentFillVertically(JPanel(), 0)
            .panel
    }

    override fun isModified(): Boolean =
        serverUrlField?.text != CkbSettings.instance.serverUrl

    override fun apply() {
        CkbSettings.instance.serverUrl = serverUrlField?.text ?: "http://localhost:3000"
    }

    override fun reset() {
        serverUrlField?.text = CkbSettings.instance.serverUrl
    }
}
