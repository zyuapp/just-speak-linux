import QtQuick
import QtQuick.Shapes

// Original JustSpeak speech bubble and sound bars; MIT, like the application.
// Draw the small monochrome mark directly so it follows the bar's theme.
Item {
    id: root
    property color color: "white"
    implicitWidth: 24
    implicitHeight: 24

    Item {
        width: 24
        height: 24
        anchors.centerIn: parent
        scale: Math.min(root.width, root.height) / 24

        Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
                fillColor: "transparent"
                strokeColor: root.color
                strokeWidth: 1.8
                capStyle: ShapePath.RoundCap
                joinStyle: ShapePath.RoundJoin
                PathSvg { path: "M6 3H18A3 3 0 0 1 21 6V16A3 3 0 0 1 18 19H11L6 23V19A3 3 0 0 1 3 16V6A3 3 0 0 1 6 3Z" }
            }
            ShapePath {
                fillColor: "transparent"
                strokeColor: root.color
                strokeWidth: 1.8
                capStyle: ShapePath.RoundCap
                PathSvg { path: "M8 10V12M12 7V15M16 9V13" }
            }
        }
    }
}
