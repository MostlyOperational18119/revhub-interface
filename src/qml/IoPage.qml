import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import revhub_interface

BasePage {
    id: ioView
    objectName: "io"
    title: "Digital & Analog IO"

    SectionCard {
        Layout.fillWidth: true
        header: Kirigami.Heading { text: "Analog inputs"; level: 3 }
        body: GridLayout {
            columns: 3
            columnSpacing: Kirigami.Units.largeSpacing * 2
            Repeater {
                model: 4
                delegate: Item {
                    id: analog
                    required property int index
                    readonly property real volts: ioView.hub ? ioView.hub.analog[index] : 0
                    // Three cells per row: label, bar, value.
                    Layout.columnSpan: 3
                    Layout.fillWidth: true
                    implicitHeight: row.implicitHeight
                    RowLayout {
                        id: row
                        anchors.fill: parent
                        spacing: Kirigami.Units.largeSpacing * 2
                        QQC2.Label { text: "Analog " + analog.index; Layout.preferredWidth: Kirigami.Units.gridUnit * 4 }
                        Meter {
                            Layout.fillWidth: true
                            from: 0
                            to: 3.3
                            value: analog.volts
                        }
                        QQC2.Label {
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 5
                            horizontalAlignment: Text.AlignRight
                            text: ioView.fmt(analog.volts, 3) + " V"
                            font.family: "monospace"
                        }
                    }
                }
            }
        }
    }

    SectionCard {
        Layout.fillWidth: true
        header: Kirigami.Heading { text: "Digital pins"; level: 3 }
        body: GridLayout {
            columns: Math.max(1, Math.floor(width / (Kirigami.Units.gridUnit * 18)))
            columnSpacing: Kirigami.Units.largeSpacing * 3
            rowSpacing: Kirigami.Units.largeSpacing
            Repeater {
                model: 8
                delegate: RowLayout {
                    id: pin
                    required property int index
                    readonly property var d: ioView.hub ? ioView.hub.dio[index] : ({})
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.largeSpacing

                    QQC2.Label { text: "Pin " + pin.index; Layout.preferredWidth: Kirigami.Units.gridUnit * 3 }
                    QQC2.ButtonGroup { id: dirGroup }
                    QQC2.Button {
                        text: "In"
                        checkable: true
                        checked: pin.d.output !== true
                        QQC2.ButtonGroup.group: dirGroup
                        onClicked: Backend.setDioDirection(ioView.hubIndex, pin.index, false)
                    }
                    QQC2.Button {
                        text: "Out"
                        checkable: true
                        checked: pin.d.output === true
                        QQC2.ButtonGroup.group: dirGroup
                        onClicked: Backend.setDioDirection(ioView.hubIndex, pin.index, true)
                    }
                    Item { Layout.fillWidth: true }
                    QQC2.Switch {
                        visible: pin.d.output === true
                        checked: pin.d.value === true
                        text: checked ? "High" : "Low"
                        onToggled: Backend.setDioOutput(ioView.hubIndex, pin.index, checked)
                    }
                    RowLayout {
                        visible: pin.d.output !== true
                        Rectangle {
                            implicitWidth: Kirigami.Units.gridUnit * 0.8
                            implicitHeight: implicitWidth
                            radius: width / 2
                            color: pin.d.value ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                        }
                        QQC2.Label { text: pin.d.value ? "High" : "Low" }
                    }
                }
            }
        }
    }
}
