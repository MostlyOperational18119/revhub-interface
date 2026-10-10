import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import revhub_interface

BasePage {
    id: motorsView
    objectName: "motors"
    title: "DC Motors"

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: motorsView.hub !== null && motorsView.hub.battery < 7
        type: Kirigami.MessageType.Warning
        text: "No battery detected. Motors and servos need 12 V battery power to move."
    }

    GridLayout {
        Layout.fillWidth: true
        columns: Math.max(1, Math.floor(width / (Kirigami.Units.gridUnit * 22)))
        columnSpacing: Kirigami.Units.largeSpacing * 2
        rowSpacing: Kirigami.Units.largeSpacing * 2

        Repeater {
            model: 4
            delegate: SectionCard {
                id: card
                required property int index
                readonly property var m: motorsView.hub ? motorsView.hub.motors[index] : ({})
                readonly property real power: (m.power || 0) / 32767

                Layout.fillWidth: true
                Layout.alignment: Qt.AlignTop

                function apply(value) {
                    Backend.setMotorPower(motorsView.hubIndex, card.index, Math.max(-1, Math.min(1, value)))
                }

                header: RowLayout {
                    Kirigami.Heading {
                        Layout.fillWidth: true
                        level: 3
                        text: "Motor " + card.index
                        padding: Kirigami.Units.smallSpacing
                    }
                    Kirigami.Chip {
                        checkable: false
                        closable: false
                        text: card.m.enabled ? (card.power === 0 ? "Holding" : "Running") : "Off"
                        icon.name: card.m.enabled && card.power !== 0 ? "media-playback-start" : "media-playback-pause"
                    }
                }

                body: ColumnLayout {
                    spacing: Kirigami.Units.largeSpacing

                    RowLayout {
                        Layout.fillWidth: true
                        QQC2.Slider {
                            id: slider
                            Layout.fillWidth: true
                            from: -1
                            to: 1
                            stepSize: 0.01
                            onMoved: card.apply(value)
                            Binding on value {
                                when: !slider.pressed
                                value: card.power
                                restoreMode: Binding.RestoreNone
                            }
                        }
                        QQC2.TextField {
                            id: field
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 4
                            horizontalAlignment: Text.AlignRight
                            validator: DoubleValidator { bottom: -1; top: 1; decimals: 2; notation: DoubleValidator.StandardNotation }
                            onAccepted: card.apply(Number(text))
                            Binding on text {
                                when: !field.activeFocus
                                value: card.power.toFixed(2)
                                restoreMode: Binding.RestoreNone
                            }
                            QQC2.ToolTip.text: "Power, -1.00 to 1.00 (Enter to apply)"
                            QQC2.ToolTip.visible: hovered
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        QQC2.Button {
                            text: "Stop"
                            icon.name: "media-playback-stop"
                            onClicked: card.apply(0)
                        }
                        QQC2.Switch {
                            text: "Brake at zero"
                            checked: card.m.float === false
                            onToggled: Backend.setMotorFloat(motorsView.hubIndex, card.index, !checked)
                        }
                        Item { Layout.fillWidth: true }
                        QQC2.ToolButton {
                            icon.name: "edit-reset"
                            text: "Reset encoder"
                            display: QQC2.AbstractButton.IconOnly
                            onClicked: Backend.resetEncoder(motorsView.hubIndex, card.index)
                            QQC2.ToolTip.text: text
                            QQC2.ToolTip.visible: hovered
                        }
                    }

                    Kirigami.Separator { Layout.fillWidth: true }

                    GridLayout {
                        Layout.fillWidth: true
                        columns: 3
                        uniformCellWidths: true
                        Repeater {
                            model: [
                                { label: "Encoder", value: String(card.m.position ?? "–"), unit: "ticks" },
                                { label: "Velocity", value: String(card.m.velocity ?? "–"), unit: "ticks/s" },
                                { label: "Current", value: String(card.m.current ?? "–"), unit: "mA" }
                            ]
                            delegate: ColumnLayout {
                                required property var modelData
                                Layout.fillWidth: true
                                spacing: 0
                                QQC2.Label {
                                    text: modelData.label
                                    font: Kirigami.Theme.smallFont
                                    color: Kirigami.Theme.disabledTextColor
                                }
                                QQC2.Label {
                                    text: modelData.value + " " + modelData.unit
                                    font.family: "monospace"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
