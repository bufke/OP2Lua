' MissionTypes.vb
'
' Data types for the OP2Lua region/unit editing layer. Kept as top-level types in their OWN file
' (NOT nested inside the fMain Form partial) so the WinForms designer never inspects them when it
' loads fMain - nested public types in a Form partial make the designer fail to resolve the base class.

Public Enum RegionKind
    Rectangle
    Point
End Enum

' A named region: a rectangle (x1,y1)-(x2,y2) or a single point. 1-based tile coords (status-bar /
' placement.lua space).
Public Class MissionRegion
    Public Name As String
    Public Kind As RegionKind
    Public X1 As Integer
    Public Y1 As Integer
    Public X2 As Integer
    Public Y2 As Integer

    Public Overrides Function ToString() As String
        If Kind = RegionKind.Point Then
            Return Name & "  (" & X1 & "," & Y1 & ")"
        End If
        Return Name & "  (" & X1 & "," & Y1 & ")-(" & X2 & "," & Y2 & ")"
    End Function
End Class

' A unit parsed from placement.lua, drawn as a labelled placeholder (no sprites) for context.
' The entry's own text is kept (Raw, Lead) so saving writes back every field the editor doesn't edit
' (direction, health, lights, kit, cargo, ...) and the comments around it; only the fields the editor
' changed (at, type, player, weapon) are rewritten.
Public Class MissionUnit
    Public UnitType As String
    Public Player As Integer
    Public X As Integer        ' 1-based tile coords
    Public Y As Integer
    Public HasAt As Boolean = True   ' False: no plain { x, y } (e.g. a raw location); kept, not drawn
    Public Name As String
    Public Weapon As String    ' optional, e.g. "Laser" ("" = none)

    ' Round-trip state; Raw is Nothing for a unit placed in the editor.
    Public Raw As String             ' the text between the entry's braces
    Public Lead As String            ' the text before the entry (comments, indentation), separator comma removed
    Public OrigIndex As Integer = -1 ' position in the file's units table when loaded
    Public OrigType As String
    Public OrigPlayer As Integer
    Public OrigX As Integer
    Public OrigY As Integer
    Public OrigWeapon As String

    Public Overrides Function ToString() As String
        Dim w As String = If(String.IsNullOrEmpty(Weapon), "", " " & Weapon)
        Dim n As String = If(String.IsNullOrEmpty(Name), "", "  """ & Name & """")
        If Not HasAt Then Return UnitType & w & "  p" & Player & " (not on the map)" & n
        Return UnitType & w & "  p" & Player & " (" & X & "," & Y & ")" & n
    End Function
End Class

Public Enum MapObjectKind
    Beacon
    Wall
    Marker
End Enum

' A beacon, wall/tube or marker from placement.lua, shown on the map for context (read-only: the editor
' leaves those tables as they are in the file).
Public Class MissionMapObject
    Public Kind As MapObjectKind
    Public ObjType As String   ' beacon: MiningBeacon / MagmaVent / Fumarole; wall: Tube / Wall / ...; marker: Circle / DNA / Beaker
    Public Ore As String       ' a mining beacon's ore: Common / Rare / Random
    Public X As Integer        ' 1-based tile coords
    Public Y As Integer
    Public Name As String
End Class

' What an in-progress mouse drag is doing to a region.
Friend Enum EditAction
    None
    Drawing      ' rubber-banding a new region
    Moving       ' dragging an existing region's body
    NW           ' dragging a corner/edge handle (resize)
    N
    NE
    E
    SE
    S
    SW
    W
End Enum
