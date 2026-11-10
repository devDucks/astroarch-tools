import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.i18n.localeData
import AstroArchControlCenter

// The Language page's "Change…": country, language and keyboard of the locale module
// (its LocaleBridge, compiled in from crates/astroarch-locale-module: the same code, not
// a copy), as one wizard. Nothing is applied before "Apply"; the keyboard layout is tried
// in the session only when asked, and put back unless it is applied
ColumnLayout {
    id: view

    // Light radio buttons, as in KioskSetup: the Plasma style draws them with the dark theme
    // (an unchecked one looks checked on the light card)
    component CardRadio: RadioButton {
        id: radio
        indicator: Rectangle {
            x: radio.leftPadding
            y: radio.topPadding + (radio.availableHeight - height) / 2
            implicitWidth: 20
            implicitHeight: 20
            radius: 10
            color: "#ffffff"
            border.width: 2
            border.color: radio.visualFocus ? "#3daee9" : (radio.checked ? "#1d6fa5" : "#7d8994")
            Rectangle {
                anchors.centerIn: parent
                width: 10
                height: 10
                radius: 5
                color: "#1d6fa5"
                visible: radio.checked
            }
        }
    }

    property var probes: ({})
    property Item navHost: null
    signal systemChanged()
    signal scrollToTop()

    readonly property bool navShown: view.mode === "wizard" || view.message !== ""
    readonly property bool ownsPrimary: view.mode === "wizard"

    property string mode: "start"          // "start" | "wizard"
    property int step: 1                   // 1 country, 2 language, 3 keyboard, 4 apply
    property bool suggested: false         // the country is the one of the network
    property bool busy: false
    property bool otherLanguage: false     // the whole list of languages, not the country's
    property bool otherKeyboard: false     // the whole list of layouts, not the proposed ones
    property string message: ""
    property bool warning: false

    readonly property var steps: [qsTr("Country"), qsTr("Language"), qsTr("Keyboard"), qsTr("Apply")]

    spacing: 12

    // A new LocaleBridge at each start: it then holds what the system uses now, or the
    // module's own defaults, never a choice left from a cancelled wizard
    Component {
        id: bridgeMaker
        LocaleBridge {}
    }
    property LocaleBridge bridge: bridgeMaker.createObject(view)

    // ------------------------------------------------------------------ names
    // Names from Qt's locale data, each in its own language: "Italiano (Italia)"
    // (as the locale module's language page names them)
    function languageName(code) {
        // "sr_RS@latin": Qt writes the Latin script as sr_Latn_RS; other variants
        // ("ca_ES@valencia") are named after the base locale, with the variant
        const [base, variant] = code.split("@")
        const [language, country] = base.split("_")
        const qtName = variant === "latin" && country ? language + "_Latn_" + country : base
        const l = Qt.locale(qtName)
        if (l.name === "C")
            return code
        const lang = l.nativeLanguageName.charAt(0).toUpperCase() + l.nativeLanguageName.slice(1)
        const where = country ? " (" + (l.name === qtName && l.nativeTerritoryName ? l.nativeTerritoryName : country) + ")" : ""
        return lang + where + (variant && variant !== "latin" ? " · " + variant : "")
    }
    // A country in the language of the session, and in its own language as detail
    function nativeCountryName(code, locales) {
        for (const loc of locales.split(",")) {
            const l = Qt.locale(loc)
            if (l.name === loc && l.nativeTerritoryName)
                return l.nativeTerritoryName
        }
        return code
    }

    readonly property var countries: Array.from(view.bridge.country_codes).map((code, i) => {
        const native = view.nativeCountryName(code, view.bridge.country_locales[i] || "")
        const name = Country.fromAlpha2(code).name || native
        return { code: code, name: name, detail: native !== name ? native : "" }
    }).sort((a, b) => a.name.localeCompare(b.name))
    readonly property var languages: Array.from(view.bridge.language_codes)
        .map(code => ({ code: code, name: view.languageName(code), detail: code }))
        .sort((a, b) => a.name.localeCompare(b.name))
    readonly property var layouts: Array.from(view.bridge.keyboard_layout_codes)
        .map((code, i) => ({ code: code, name: view.bridge.keyboard_layout_names[i] || code, detail: code }))
        .sort((a, b) => a.name.localeCompare(b.name))
    readonly property var variants: [{ code: "", name: qsTr("Default"), detail: "" }].concat(
        Array.from(view.bridge.keyboard_variant_codes)
            .map((code, i) => ({ code: code, name: view.bridge.keyboard_variant_names[i] || code, detail: "" })))

    // Languages of the chosen country (restcountries.com, else the installed locales)
    readonly property var countryLanguages: Array.from(view.bridge.country_language_codes)
        .map(code => ({ code: code, name: view.languageName(code), detail: "" }))
    readonly property string countryName: view.nameIn(view.countries, view.bridge.selected_country_code)
    // The language is chosen: one of the country's, or any from the whole list
    readonly property bool showCountryLanguages: view.bridge.selected_country_code !== "" && !view.otherLanguage
        && (view.countryLanguages.length > 0 || view.bridge.languages_source === "loading")
    readonly property bool languagePicked: view.bridge.languages_source !== "loading" && (!view.showCountryLanguages
        || view.countryLanguages.some(l => l.code === view.bridge.selected_language_code))
    // Keyboards proposed for the country and the language
    readonly property var keyboardProposals: Array.from(view.bridge.keyboard_suggestion_layouts)
        .map((layout, i) => ({ layout: layout, variant: view.bridge.keyboard_suggestion_variants[i] || "",
                               name: view.bridge.keyboard_suggestion_names[i] || layout }))
    readonly property bool proposalSelected: view.keyboardProposals.some(p =>
        p.layout === view.bridge.selected_layout_code && p.variant === view.bridge.selected_variant_code)
    // The whole list of layouts: asked for, or the keyboard is none of the proposed ones
    readonly property bool keyboardListShown: view.otherKeyboard || view.keyboardProposals.length === 0
        || !view.proposalSelected

    function nameIn(list, code) {
        const e = list.find(i => i.code === code)
        return e ? e.name : code
    }
    readonly property string keyboardText: {
        const layout = view.nameIn(view.layouts, view.bridge.selected_layout_code)
        return view.bridge.selected_variant_code === "" ? layout
             : layout + " · " + view.nameIn(view.variants, view.bridge.selected_variant_code)
    }

    // ------------------------------------------------------------------ flow
    // From a row of the page: "country" (the country of the network, pre-selected),
    // "language" or "keyboard". One wizard: the other steps are a Back / Next away
    function start(what) {
        // What the system uses now. No lookup of its own: the page's probe already found
        // the country
        const old = view.bridge
        view.bridge = bridgeMaker.createObject(view)
        old.destroy()
        view.bridge.initialize(false)
        view.message = ""
        view.warning = false
        view.otherLanguage = false
        view.otherKeyboard = false
        for (const list of [countryPick, languagePick, layoutPick, variantPick])
            list.clear()
        // The country of the network (probe country-suggest, found only while language,
        // keyboard and time zone are the image defaults): it chooses language and keyboard
        const suggestion = view.probes["country-suggest"]
        view.suggested = what === "country" && suggestion !== undefined && suggestion.ok
        if (view.suggested)
            view.bridge.set_country(suggestion.value.split("|")[0])
        view.step = what === "keyboard" ? 3 : (what === "language" ? 2 : 1)
        view.mode = "wizard"
        view.scrollToTop()
    }

    // The session layout as it was before "Try this layout", if one was tried and not
    // applied (the module keeps the whole layout of the session: several layouts, their
    // switch, kxkbrc on Wayland). Nothing to do otherwise
    function untry() {
        view.bridge.restore_keyboard_if_unsaved()
    }

    function cancel() {
        view.untry()
        view.mode = "start"
        view.message = ""
        view.scrollToTop()
    }

    function back() {
        if (view.step > 1)
            view.step--
        view.scrollToTop()
    }

    function next() {
        if (view.step < 4) {
            view.step++
            view.scrollToTop()
            return
        }
        // Drawn before the window waits for the password and the commands
        view.busy = true
        view.message = qsTr("Applying… Your password may be asked.")
        view.warning = false
        applyLater.start()
    }

    Timer {
        id: applyLater
        interval: 100
        onTriggered: {
            const ok = view.bridge.persist_configuration()
            view.busy = false
            if (ok) {
                view.mode = "start"
                view.message = qsTr("Applied. Log out and back in for the new language to apply everywhere.")
                view.systemChanged()
            } else {
                // The module changes no keyboard when it fails; a layout being tried goes back
                view.untry()
                view.warning = true
                view.message = qsTr("Not applied: the password was refused or the system commands failed.")
            }
        }
    }

    // The languages of the country come from restcountries.com: read when they are there
    Timer {
        interval: 200
        repeat: true
        running: view.bridge.languages_source === "loading"
        onTriggered: view.bridge.poll_country_languages()
    }

    // A layout being tried goes back when the page is left (another page shown) or the
    // window closes without "Apply"
    onVisibleChanged: if (!visible) view.untry()
    Connections {
        target: view.Window.window
        function onClosing() {
            view.untry()
        }
    }

    // =============================================================== WIZARD
    ColumnLayout {
        Layout.fillWidth: true
        visible: view.mode === "wizard"
        spacing: 12

        WizardSteps {
            steps: view.steps
            step: view.step
        }

        // ---- Step 1: the country, which chooses language and keyboard
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 1
            spacing: 10
            Label {
                Layout.fillWidth: true
                text: view.suggested && view.bridge.selected_country_code !== ""
                      ? qsTr("The country of your network is proposed. Its languages and keyboards are proposed in the next steps.")
                      : qsTr("Choose your country: its languages and keyboards are proposed in the next steps. You can also skip this step.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
            PickList {
                id: countryPick
                Layout.fillWidth: true
                items: view.countries
                current: view.bridge.selected_country_code
                accessibleName: qsTr("Country")
                placeholder: qsTr("Search a country…")
                onPicked: (code) => {
                    view.bridge.set_country(code)
                    view.suggested = false
                }
            }
        }

        // ---- Step 2: the language: the country's, or any other
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 2
            spacing: 10
            Label {
                Layout.fillWidth: true
                text: qsTr("The language of the menus and programs, and the formats of dates and numbers.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }

            // The country's languages: one is chosen from them
            ColumnLayout {
                Layout.fillWidth: true
                visible: view.showCountryLanguages
                spacing: 6
                Label {
                    Layout.fillWidth: true
                    visible: view.bridge.languages_source === "loading"
                    text: qsTr("Looking up the languages of %1…").arg(view.countryName)
                    color: "#4f5a64"
                    wrapMode: Text.Wrap
                }
                Label {
                    Layout.fillWidth: true
                    visible: view.countryLanguages.length > 0
                    text: qsTr("Languages of %1: choose yours.").arg(view.countryName)
                    color: "#4f5a64"
                    font.bold: true
                    wrapMode: Text.Wrap
                }
                Label {
                    Layout.fillWidth: true
                    visible: view.bridge.languages_source === "offline" && view.countryLanguages.length > 0
                    text: qsTr("No connection: these are the languages installed for %1.").arg(view.countryName)
                    color: "#b58900"
                    wrapMode: Text.Wrap
                }
                Repeater {
                    model: view.countryLanguages
                    CardRadio {
                        required property var modelData
                        Layout.fillWidth: true
                        text: modelData.name
                        checked: modelData.code === view.bridge.selected_language_code
                        onClicked: view.bridge.set_language(modelData.code)
                    }
                }
                CardButton {
                    visible: view.bridge.languages_source !== "loading"
                    text: qsTr("Another language…")
                    onClicked: view.otherLanguage = true
                }
            }

            // Any language: no country chosen, none of the country's, or a foreign one
            ColumnLayout {
                Layout.fillWidth: true
                visible: !view.showCountryLanguages
                spacing: 6
                CardButton {
                    visible: view.otherLanguage && view.countryLanguages.length > 0
                    text: qsTr("Back to the languages of %1").arg(view.countryName)
                    onClicked: view.otherLanguage = false
                }
                PickList {
                    id: languagePick
                    Layout.fillWidth: true
                    items: view.languages
                    current: view.bridge.selected_language_code
                    accessibleName: qsTr("Language")
                    placeholder: qsTr("Search a language…")
                    onPicked: (code) => view.bridge.set_language(code)
                }
            }
        }

        // ---- Step 3: the keyboard, and a field to try it
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 3
            spacing: 10

            // Proposed for the country and the language: one is chosen, or another keyboard
            // (a keyboard bought in another country) from the whole list
            ColumnLayout {
                Layout.fillWidth: true
                visible: view.keyboardProposals.length > 0
                spacing: 6
                Label {
                    Layout.fillWidth: true
                    text: qsTr("Proposed for your country and language.")
                    color: "#4f5a64"
                    font.bold: true
                    wrapMode: Text.Wrap
                }
                Repeater {
                    model: view.keyboardProposals
                    CardRadio {
                        required property var modelData
                        Layout.fillWidth: true
                        text: modelData.name
                        checked: modelData.layout === view.bridge.selected_layout_code
                                 && modelData.variant === view.bridge.selected_variant_code
                        onClicked: {
                            view.bridge.choose_keyboard(modelData.layout, modelData.variant)
                            view.otherKeyboard = false
                        }
                    }
                }
                CardButton {
                    visible: !view.keyboardListShown
                    text: qsTr("Another keyboard…")
                    onClicked: view.otherKeyboard = true
                }
                CardButton {
                    visible: view.keyboardListShown && view.otherKeyboard && view.proposalSelected
                    text: qsTr("Back to the proposed keyboards")
                    onClicked: view.otherKeyboard = false
                }
            }

            GridLayout {
                visible: view.keyboardListShown
                Layout.fillWidth: true
                columns: view.width >= 560 ? 2 : 1
                columnSpacing: 16
                rowSpacing: 10

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 3
                    Layout.alignment: Qt.AlignTop
                    spacing: 6
                    Label {
                        text: qsTr("Layout")
                        color: "#4f5a64"
                        font.bold: true
                    }
                    PickList {
                        id: layoutPick
                        Layout.fillWidth: true
                        items: view.layouts
                        current: view.bridge.selected_layout_code
                        listHeight: 150
                        accessibleName: qsTr("Layout")
                        placeholder: qsTr("Search a layout…")
                        onPicked: (code) => view.bridge.set_keyboard_layout(code)
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 2
                    Layout.alignment: Qt.AlignTop
                    spacing: 6
                    Label {
                        text: qsTr("Variant")
                        color: "#4f5a64"
                        font.bold: true
                    }
                    PickList {
                        id: variantPick
                        Layout.fillWidth: true
                        items: view.variants
                        current: view.bridge.selected_variant_code
                        searchable: view.variants.length > 8
                        listHeight: 150
                        accessibleName: qsTr("Variant")
                        placeholder: qsTr("Search a variant…")
                        onPicked: (code) => view.bridge.set_keyboard_variant(code)
                    }
                }
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("To test it, try the layout, then type in the field. It is used in this session only, and put back unless you apply it.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                CardField {
                    Layout.fillWidth: true
                    placeholderText: qsTr("Type here to test the keyboard")
                    Accessible.name: placeholderText
                }
                CardButton {
                    text: qsTr("Try this layout")
                    onClicked: {
                        if (!view.bridge.apply_keyboard_for_testing()) {
                            view.warning = true
                            view.message = qsTr("The layout could not be tried in this session.")
                        } else {
                            view.message = ""
                        }
                    }
                }
            }
        }

        // ---- Step 4: what will be applied
        ColumnLayout {
            Layout.fillWidth: true
            visible: view.step === 4
            spacing: 10
            RowsBox {
                Repeater {
                    model: [
                        { label: qsTr("Language"), value: view.languageName(view.bridge.selected_language_code) },
                        { label: qsTr("Keyboard"), value: view.keyboardText }
                    ]
                    ColumnLayout {
                        id: summaryRow
                        required property var modelData
                        required property int index
                        Layout.fillWidth: true
                        spacing: 0
                        Rectangle {
                            visible: summaryRow.index > 0
                            Layout.fillWidth: true
                            implicitHeight: 1
                            color: "#e1e6ea"
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            Layout.margins: 12
                            spacing: 16
                            Label {
                                Layout.preferredWidth: 130
                                text: summaryRow.modelData.label
                                color: "#4f5a64"
                                font.bold: true
                                wrapMode: Text.Wrap
                            }
                            Label {
                                Layout.fillWidth: true
                                text: summaryRow.modelData.value
                                color: "#1f2d3a"
                                wrapMode: Text.Wrap
                            }
                        }
                    }
                }
            }
            Label {
                Layout.fillWidth: true
                text: qsTr("“Apply” sets them for the whole system and asks for your password. Log out and back in afterwards: open programs keep their language until then.")
                color: "#34495e"
                wrapMode: Text.Wrap
            }
        }
    }

    // The message and the wizard's buttons, pinned under the page (Main.qml's navHost)
    ColumnLayout {
        id: navBar
        Layout.fillWidth: true
        spacing: 10
        visible: view.visible && view.navShown
        Component.onCompleted: if (view.navHost) parent = view.navHost

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: view.message !== ""
            type: view.warning ? Kirigami.MessageType.Warning
                 : (view.mode === "start" ? Kirigami.MessageType.Positive : Kirigami.MessageType.Information)
            text: view.message
        }
        RowLayout {
            Layout.fillWidth: true
            visible: view.mode === "wizard"
            spacing: 10
            CardButton {
                enabled: !view.busy
                text: qsTr("Cancel")
                onClicked: view.cancel()
            }
            CardButton {
                visible: view.step > 1
                enabled: !view.busy
                text: qsTr("Back")
                onClicked: view.back()
            }
            Item { Layout.fillWidth: true }
            CardButton {
                highlighted: true
                // With the country's languages shown, one of them has to be chosen
                enabled: !view.busy && (view.step !== 2 || view.languagePicked)
                text: view.step === 4 ? qsTr("Apply") : qsTr("Next")
                onClicked: view.next()
            }
        }
    }
}
