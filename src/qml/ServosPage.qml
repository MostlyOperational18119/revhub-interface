import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import revhub_interface

BasePage {
    id: servosView
    objectName: "servos"
    title: "Servos"

    QQC2.Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        color: Kirigami.Theme.disabledTextColor
        text: "Pulse width 500–2500 µs at a 20 ms frame. Position is the FTC SDK value: 0.0 = 500 µs, 0.5 = 1500 µs, 1.0 = 2500 µs. Moving a control enables the servo."
    }

    GridLayout {
        Layout.fillWidth: true
        columns: Math.max(1, Math.floor(width / (Kirigami.Units.gridUnit * 20)))
        columnSpacing: Kirigami.Units.largeSpacing * 2
        rowSpacing: Kirigami.Units.largeSpacing * 2

        Repeater {
            model: 6
            delegate: SectionCard {
                id: card
                required property int index
                readonly property var s: servosView.hub ? servosView.hub.servos[index] : ({})
                readonly property int pulse: s.pulse || 1500

                Layout.fillWidth: true
                Layout.alignment: Qt.AlignTop

                function apply(pulseUs, enabled) {
                    Backend.setServo(servosView.hubIndex, card.index, Math.round(Math.max(500, Math.min(2500, pulseUs))), enabled)
                }

                header: RowLayout {
                    Kirigami.Heading {
                        Layout.fillWidth: true
                        level: 3
                        text: "Servo " + card.index
                        padding: Kirigami.Units.smallSpacing
                    }
                    QQC2.Switch {
                        text: "Enabled"
                        checked: card.s.enabled === true
                        onToggled: card.apply(card.pulse, checked)
                    }
                }

                body: ColumnLayout {
                    spacing: Kirigami.Units.largeSpacing

                    QQC2.Slider {
                        id: slider
                        Layout.fillWidth: true
                        from: 500
                        to: 2500
                        stepSize: 10
                        onMoved: card.apply(value, true)
                        Binding on value {
                            when: !slider.pressed
                            value: card.pulse
                            restoreMode: Binding.RestoreNone
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        QQC2.Label { text: "Position" }
                        QQC2.TextField {
                            id: posField
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 4
                            horizontalAlignment: Text.AlignRight
                            validator: DoubleValidator { bottom: 0; top: 1; decimals: 3; notation: DoubleValidator.StandardNotation }
                            onAccepted: card.apply(500 + Number(text) * 2000, true)
                            Binding on text {
                                when: !posField.activeFocus
                                value: ((card.pulse - 500) / 2000).toFixed(3)
                                restoreMode: Binding.RestoreNone
                            }
                        }
                        QQC2.Label { text: "Pulse" }
                        QQC2.TextField {
                            id: usField
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 4
                            horizontalAlignment: Text.AlignRight
                            validator: IntValidator { bottom: 500; top: 2500 }
                            onAccepted: card.apply(Number(text), true)
                            Binding on text {
                                when: !usField.activeFocus
                                value: String(card.pulse)
                                restoreMode: Binding.RestoreNone
                            }
                        }
                        QQC2.Label { text: "µs" }
                        Item { Layout.fillWidth: true }
                        QQC2.Button {
                            text: "Center"
                            onClicked: card.apply(1500, true)
                        }
                    }
                }
            }
        }
    }
}
