import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import revhub_interface

BasePage {
    id: sensorsView
    objectName: "sensors"
    title: "I2C Sensors"

    component Reading: ColumnLayout {
        id: reading
        property string kind: ""
        property var value: null

        spacing: Kirigami.Units.smallSpacing

        QQC2.Label {
            visible: reading.value !== null && reading.value.error !== undefined
            text: reading.value && reading.value.error ? "⚠ " + reading.value.error : ""
            color: Kirigami.Theme.negativeTextColor
        }

        // REV 2m Distance Sensor
        Kirigami.Heading {
            visible: reading.kind === "Distance2m" && reading.value !== null && reading.value.error === undefined
            level: 1
            text: reading.value && reading.value.distance_mm !== null && reading.value.distance_mm !== undefined
                ? reading.value.distance_mm + " mm" : "Out of range"
        }

        // Color sensors
        RowLayout {
            visible: (reading.kind === "ColorV3" || reading.kind === "ColorV2") && reading.value !== null && reading.value.error === undefined
            spacing: Kirigami.Units.largeSpacing
            Rectangle {
                implicitWidth: Kirigami.Units.gridUnit * 3
                implicitHeight: implicitWidth
                radius: Kirigami.Units.cornerRadius
                color: reading.value && reading.value.swatch ? reading.value.swatch : "transparent"
                border.color: Kirigami.Theme.disabledTextColor
            }
            GridLayout {
                columns: 2
                columnSpacing: Kirigami.Units.largeSpacing
                rowSpacing: 0
                Repeater {
                    model: [["Red", "red"], ["Green", "green"], ["Blue", "blue"], ["Clear", "clear"], ["Proximity", "proximity"]]
                    delegate: QQC2.Label {
                        required property var modelData
                        Layout.columnSpan: modelData[1] === "proximity" ? 2 : 1
                        text: modelData[0] + ": " + (reading.value ? reading.value[modelData[1]] : "–")
                        font.family: "monospace"
                    }
                }
            }
        }

        // IMU
        GridLayout {
            visible: reading.kind === "Imu" && reading.value !== null
            columns: 2
            columnSpacing: Kirigami.Units.largeSpacing * 2
            rowSpacing: 0
            Repeater {
                model: ["heading", "roll", "pitch"]
                delegate: QQC2.Label {
                    required property string modelData
                    Layout.columnSpan: 2
                    text: modelData.charAt(0).toUpperCase() + modelData.slice(1) + ": "
                        + (reading.value ? sensorsView.fmt(reading.value[modelData], 2) : "–") + "°"
                    font.family: "monospace"
                }
            }
            QQC2.Label {
                Layout.columnSpan: 2
                text: reading.value && reading.value.gravity
                    ? "Gravity (m/s²): X " + sensorsView.fmt(reading.value.gravity[0], 2) + "  Y " + sensorsView.fmt(reading.value.gravity[1], 2) + "  Z " + sensorsView.fmt(reading.value.gravity[2], 2)
                    : ""
                font.family: "monospace"
            }
        }
    }

    SectionCard {
        id: imuCard
        readonly property var slot: sensorsView.hub ? sensorsView.hub.imu : ({})
        Layout.fillWidth: true

        header: RowLayout {
            Kirigami.Heading {
                Layout.fillWidth: true
                level: 3
                text: "Built-in IMU (I2C bus 0)"
                padding: Kirigami.Units.smallSpacing
            }
            QQC2.Button {
                text: "Initialize"
                icon.name: "view-refresh"
                onClicked: Backend.imuInit(sensorsView.hubIndex)
            }
            QQC2.Switch {
                text: "Poll"
                enabled: !!imuCard.slot.device
                checked: imuCard.slot.polling === true
                onToggled: Backend.imuPoll(sensorsView.hubIndex, checked)
            }
        }
        body: ColumnLayout {
            QQC2.Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: imuCard.slot.message || "Press Initialize to start the BNO055 in IMU fusion mode."
                color: Kirigami.Theme.disabledTextColor
            }
            Reading {
                kind: imuCard.slot.kind || ""
                value: imuCard.slot.reading || null
            }
        }
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
                readonly property var slot: sensorsView.hub ? sensorsView.hub.i2c[index] : ({})

                Layout.fillWidth: true
                Layout.fillHeight: true

                header: RowLayout {
                    Kirigami.Heading {
                        Layout.fillWidth: true
                        level: 3
                        text: "I2C bus " + card.index
                        padding: Kirigami.Units.smallSpacing
                    }
                    QQC2.Button {
                        text: "Detect"
                        icon.name: "edit-find"
                        onClicked: Backend.i2cDetect(sensorsView.hubIndex, card.index)
                    }
                    QQC2.Switch {
                        text: "Poll"
                        enabled: !!card.slot.device
                        checked: card.slot.polling === true
                        onToggled: Backend.i2cPoll(sensorsView.hubIndex, card.index, checked)
                    }
                }
                body: ColumnLayout {
                    QQC2.Label {
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        text: card.slot.message || "Supported: REV 2m Distance Sensor, Color Sensor V3 and V2."
                        color: Kirigami.Theme.disabledTextColor
                    }
                    Reading {
                        kind: card.slot.kind || ""
                        value: card.slot.reading || null
                    }
                }
            }
        }
    }
}
