import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs as Dialogs
import org.kde.kirigami as Kirigami
import revhub_interface

// Hub information, address assignment and firmware update.
BasePage {
    id: hubView
    objectName: "hub"
    title: "Hub & Firmware"
    requiresConnection: false

    property string firmwarePath: ""

    Kirigami.Heading { text: "Connected hubs"; level: 3; visible: hubView.hub !== null }

    Repeater {
        model: hubView.hubs.length
        delegate: GridLayout {
            id: info
            required property int index
            readonly property var h: hubView.hubs[index] || ({})
            Layout.fillWidth: true
            columns: 2
            columnSpacing: Kirigami.Units.largeSpacing * 2

            component Key: QQC2.Label {
                Layout.alignment: Qt.AlignRight
                color: Kirigami.Theme.disabledTextColor
            }

            QQC2.Label {
                Layout.columnSpan: 2
                font.bold: true
                text: "Hub " + info.index + (info.h.parent ? " — connected over USB" : " — RS485 daisy chain")
            }
            Key { text: "Firmware" }
            QQC2.Label { text: info.h.firmware || "–"; textFormat: Text.PlainText }
            Key { text: "Power" }
            QQC2.Label { text: hubView.fmt(info.h.battery, 2) + " V battery, " + hubView.fmt(info.h.monitor5v, 2) + " V 5V rail" }
            Key { text: "Controller temperature" }
            QQC2.Label { text: hubView.fmt(info.h.temperature, 1) + " °C" }
            Key { text: "Address" }
            RowLayout {
                QQC2.SpinBox {
                    id: addr
                    from: 1; to: 254
                    value: info.h.address || 1
                    editable: true
                }
                QQC2.Button {
                    text: "Apply"
                    icon.name: "dialog-ok-apply"
                    enabled: addr.value !== info.h.address
                    onClicked: Backend.setAddress(info.index, addr.value)
                }
            }
        }
    }

    QQC2.Label {
        Layout.fillWidth: true
        visible: hubView.hub !== null
        wrapMode: Text.Wrap
        color: Kirigami.Theme.disabledTextColor
        text: "Give every daisy-chained hub a unique address; it is stored on the hub."
    }

    Kirigami.Separator { Layout.fillWidth: true; visible: hubView.hub !== null }

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: true
        type: Kirigami.MessageType.Warning
        text: "Firmware update: update only the Expansion Hub connected directly over USB, and only with REV-provided .bin files. Incorrect firmware can brick the hub, and modified firmware is not FTC legal. Do not remove power while programming."
    }

    Kirigami.Heading { text: "Firmware update"; level: 3 }

    RowLayout {
        Layout.fillWidth: true
        QQC2.TextField {
            Layout.fillWidth: true
            readOnly: true
            placeholderText: "No firmware file selected"
            text: hubView.firmwarePath.replace(/^file:\/\//, "")
        }
        QQC2.Button {
            text: "Browse…"
            icon.name: "document-open"
            onClicked: fileDialog.open()
        }
        QQC2.Button {
            text: "Flash firmware"
            icon.name: "run-build-install"
            enabled: hubView.firmwarePath !== "" && !hubView.flashing
            onClicked: confirmFlash.open()
        }
    }

    QQC2.ProgressBar {
        Layout.fillWidth: true
        visible: hubView.flashing || Backend.flashProgress > 0
        from: 0; to: 1
        value: Backend.flashProgress
    }
    QQC2.Label {
        Layout.fillWidth: true
        visible: text !== ""
        wrapMode: Text.Wrap
        text: Backend.flashMessage
    }

    Kirigami.Separator { Layout.fillWidth: true }

    QQC2.Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        color: Kirigami.Theme.disabledTextColor
        text: "REV Hub Interface for Linux " + Backend.appVersion()
            + " · a Qt Quick / Kirigami port of REV Robotics' REV Hub Interface 1.2.0 (BSD-3-Clause)."
    }

    Dialogs.FileDialog {
        id: fileDialog
        title: "Select Expansion Hub firmware"
        nameFilters: ["Firmware images (*.bin)", "All files (*)"]
        onAccepted: hubView.firmwarePath = selectedFile.toString()
    }

    Kirigami.PromptDialog {
        id: confirmFlash
        title: "Flash firmware?"
        subtitle: "The hub will be disconnected and reprogrammed with\n" + hubView.firmwarePath.replace(/^file:\/\//, "")
            + "\n\nKeep the hub powered and connected until programming completes."
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        onAccepted: Backend.flashFirmware(hubView.firmwarePath)
    }
}
