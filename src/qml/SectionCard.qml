import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Card with a header row and a body. Unlike Kirigami.Card, its implicit height
// always follows the body's layout, including when a page is re-created.
QQC2.Control {
    id: card

    property Item header: null
    property Item body: null

    padding: Kirigami.Units.largeSpacing

    Kirigami.Theme.colorSet: Kirigami.Theme.View
    Kirigami.Theme.inherit: false

    background: Kirigami.ShadowedRectangle {
        color: Kirigami.Theme.backgroundColor
        radius: Kirigami.Units.cornerRadius
        border.width: 1
        border.color: Qt.alpha(Kirigami.Theme.textColor, 0.15)
        shadow.size: Kirigami.Units.smallSpacing
        shadow.color: Qt.alpha("black", 0.15)
        shadow.yOffset: 1
    }

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        LayoutItemProxy {
            Layout.fillWidth: true
            visible: card.header !== null
            target: card.header
        }
        Kirigami.Separator {
            Layout.fillWidth: true
            visible: card.header !== null && card.body !== null
        }
        LayoutItemProxy {
            Layout.fillWidth: true
            visible: card.body !== null
            target: card.body
        }
    }
}
