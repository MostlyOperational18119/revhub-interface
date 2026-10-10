import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import revhub_interface

Kirigami.ApplicationWindow {
    id: root

    title: "REV Hub Interface"
    width: Kirigami.Units.gridUnit * 64
    height: Kirigami.Units.gridUnit * 44
    minimumWidth: Kirigami.Units.gridUnit * 24
    minimumHeight: Kirigami.Units.gridUnit * 24

    readonly property var st: Backend.state
    readonly property bool connected: st.connected === true
    readonly property var hubs: st.hubs || []
    property int hubIndex: 0
    readonly property var hub: hubIndex < hubs.length ? hubs[hubIndex] : null

    onHubsChanged: if (hubIndex >= hubs.length) hubIndex = 0

    readonly property Kirigami.Action connectAction: Kirigami.Action {
        text: root.connected ? "Disconnect" : "Connect"
        icon.name: root.connected ? "network-disconnect" : "network-connect"
        enabled: !root.st.flashing
        onTriggered: root.connected ? Backend.disconnectHubs() : Backend.connectHubs()
    }

    readonly property Kirigami.Action stopAction: Kirigami.Action {
        text: "E-Stop"
        icon.name: "process-stop"
        tooltip: "Stop all motors and servos (Esc)"
        shortcut: "Escape"
        enabled: root.connected
        onTriggered: Backend.emergencyStop()
    }

    function formatNumber(v, digits) {
        return (v === undefined || v === null) ? "–" : Number(v).toFixed(digits)
    }

    Connections {
        target: Backend
        function onNotify(text, error) {
            root.showPassiveNotification(text, error ? "long" : "short")
        }
    }

    Component.onCompleted: { Backend.start(); showPage(motorsPage) }

    globalDrawer: Kirigami.GlobalDrawer {
        id: drawer
        isMenu: false
        modal: !root.wideScreen
        collapsible: true
        showHeaderWhenCollapsed: true

        header: Kirigami.AbstractApplicationHeader {
            contentItem: RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Kirigami.Units.largeSpacing
                anchors.rightMargin: Kirigami.Units.largeSpacing
                spacing: Kirigami.Units.smallSpacing

                Kirigami.Icon {
                    source: root.connected ? "network-connect" : "network-disconnect"
                    color: root.connected ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                    implicitWidth: Kirigami.Units.iconSizes.smallMedium
                    implicitHeight: implicitWidth
                }
                QQC2.ComboBox {
                    Layout.fillWidth: true
                    visible: !drawer.collapsed
                    enabled: root.hubs.length > 1
                    model: root.hubs.length > 0
                        ? root.hubs.map((h, i) => "Hub " + i + " (addr " + h.address + ")")
                        : [root.st.flashing ? "Updating firmware…" : "Not connected"]
                    currentIndex: root.hubs.length > 0 ? root.hubIndex : 0
                    onActivated: index => root.hubIndex = index
                }
            }
        }

        actions: [
            Kirigami.Action {
                text: "DC Motors"
                icon.name: "speedometer"
                checkable: true
                checked: root.pageStack.currentItem && root.pageStack.currentItem.objectName === "motors"
                onTriggered: root.showPage(motorsPage)
            },
            Kirigami.Action {
                text: "Servos"
                icon.name: "object-rotate-right"
                checkable: true
                checked: root.pageStack.currentItem && root.pageStack.currentItem.objectName === "servos"
                onTriggered: root.showPage(servosPage)
            },
            Kirigami.Action {
                text: "I2C Sensors"
                icon.name: "measure"
                checkable: true
                checked: root.pageStack.currentItem && root.pageStack.currentItem.objectName === "sensors"
                onTriggered: root.showPage(sensorsPage)
            },
            Kirigami.Action {
                text: "Digital & Analog IO"
                icon.name: "network-wired"
                checkable: true
                checked: root.pageStack.currentItem && root.pageStack.currentItem.objectName === "io"
                onTriggered: root.showPage(ioPage)
            },
            Kirigami.Action {
                text: "Hub & Firmware"
                icon.name: "configure"
                checkable: true
                checked: root.pageStack.currentItem && root.pageStack.currentItem.objectName === "hub"
                onTriggered: root.showPage(hubPage)
            }
        ]
    }

    function showPage(component) {
        pageStack.clear()
        pageStack.push(component)
    }

    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.ToolBar

    readonly property url motorsPage: Qt.resolvedUrl("MotorsPage.qml")
    readonly property url servosPage: Qt.resolvedUrl("ServosPage.qml")
    readonly property url sensorsPage: Qt.resolvedUrl("SensorsPage.qml")
    readonly property url ioPage: Qt.resolvedUrl("IoPage.qml")
    readonly property url hubPage: Qt.resolvedUrl("HubPage.qml")
}
