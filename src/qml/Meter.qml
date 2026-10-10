import QtQuick
import org.kde.kirigami as Kirigami

// Lightweight horizontal level meter (avoids re-laying out a full ProgressBar 10x per second).
Rectangle {
    id: meter
    property real from: 0
    property real to: 1
    property real value: 0

    implicitHeight: Kirigami.Units.smallSpacing * 2
    radius: height / 2
    color: Kirigami.Theme.alternateBackgroundColor
    border.color: Qt.alpha(Kirigami.Theme.textColor, 0.15)

    Rectangle {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: parent.width * Math.max(0, Math.min(1, (meter.value - meter.from) / (meter.to - meter.from)))
        radius: parent.radius
        color: Kirigami.Theme.highlightColor
    }
}
