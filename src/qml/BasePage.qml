import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Common page frame: connection/E-stop actions, scrolling content and a
// placeholder while no hub is connected.
Kirigami.Page {
    id: base

    property bool requiresConnection: true
    readonly property var app: QQC2.ApplicationWindow.window
    readonly property bool connected: app !== null && app.connected
    readonly property var hub: app ? app.hub : null
    readonly property int hubIndex: app ? app.hubIndex : 0
    readonly property var hubs: app ? app.hubs : []
    readonly property bool flashing: app !== null && app.st.flashing === true

    function fmt(v, digits) {
        return (v === undefined || v === null) ? "–" : Number(v).toFixed(digits)
    }
    default property alias content: column.data

    padding: 0
    actions: app ? [app.connectAction, app.stopAction] : []

    footer: QQC2.ToolBar {
        position: QQC2.ToolBar.Footer
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: Kirigami.Units.largeSpacing
            anchors.rightMargin: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.largeSpacing * 2

            QQC2.Label {
                Layout.fillWidth: true
                elide: Text.ElideRight
                text: base.flashing ? "Updating firmware…"
                    : !base.connected ? "Disconnected"
                    : base.app.st.simulated ? "Connected to simulator"
                    : "Connected · " + base.app.st.port + " (" + base.app.st.serial + ")"
                color: base.connected ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
            }
            Repeater {
                model: base.hub ? base.hub.alerts : []
                delegate: QQC2.Label {
                    required property string modelData
                    text: "⚠ " + modelData
                    color: Kirigami.Theme.negativeTextColor
                }
            }
            QQC2.Label {
                visible: base.hub !== null
                readonly property real volts: base.hub ? base.hub.battery : 0
                text: "Battery " + base.fmt(volts, 2) + " V"
                color: volts < 9 ? Kirigami.Theme.negativeTextColor
                     : volts < 11.5 ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.textColor
            }
            QQC2.Label {
                visible: base.hub !== null
                text: "5V " + base.fmt(base.hub ? base.hub.monitor5v : null, 2) + " V"
            }
            QQC2.Label {
                visible: base.hub !== null
                text: base.fmt(base.hub ? base.hub.temperature : null, 1) + " °C"
            }
        }
    }

    QQC2.ScrollView {
        id: scroll
        anchors.fill: parent
        visible: !base.requiresConnection || (base.connected && base.hub !== null)
        padding: Kirigami.Units.largeSpacing * 2
        contentWidth: availableWidth
        QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff

        ColumnLayout {
            id: column
            width: scroll.availableWidth
            spacing: Kirigami.Units.largeSpacing * 2
        }
    }

    Kirigami.PlaceholderMessage {
        anchors.centerIn: parent
        width: parent.width - Kirigami.Units.gridUnit * 4
        visible: !scroll.visible
        icon.name: base.app && base.app.st.flashing ? "run-build-install" : "network-disconnect"
        text: base.app && base.app.st.flashing ? "Firmware update in progress" : "No hub connected"
        explanation: "Plug a REV Expansion Hub into USB, power it from a 12 V battery, then press Connect."
        helpfulAction: base.app ? base.app.connectAction : null
    }
}
