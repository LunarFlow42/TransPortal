// KDE Plasma 6 KWin script. No key grabs, input injection, D-Bus bridge or
// cursor telemetry: cursor coordinates stay inside the compositor.
(function () {
    "use strict";
    var marker = /^TransPortal Overlay \[[0-9]+\]$/;

    function isOverlay(window) {
        var app = String(window.resourceClass).toLowerCase();
        return (app === "transportal" || app === "com.transportal.app") &&
            marker.test(String(window.captionNormal || window.caption));
    }

    function place(window) {
        if (!isOverlay(window) || window.minimized || window.deleted) return;
        var cursor = workspace.cursorPos;
        var screen = workspace.screenAt(cursor);
        if (!screen) return;
        var area = workspace.clientArea(KWin.PlacementArea, screen, workspace.currentDesktop);
        var geometry = window.frameGeometry;
        if (!(area.width > 0 && area.height > 0 && geometry.width > 0 && geometry.height > 0)) return;
        var gap = 12;
        var margin = 8;
        geometry.width = Math.min(geometry.width, Math.max(1, area.width - 2 * margin));
        geometry.height = Math.min(geometry.height, Math.max(1, area.height - 2 * margin));
        var x = cursor.x + gap;
        var y = cursor.y + gap;
        if (x + geometry.width > area.x + area.width - margin) x = cursor.x - geometry.width - gap;
        if (y + geometry.height > area.y + area.height - margin) y = cursor.y - geometry.height - gap;
        geometry.x = Math.max(area.x + margin, Math.min(x, area.x + area.width - geometry.width - margin));
        geometry.y = Math.max(area.y + margin, Math.min(y, area.y + area.height - geometry.height - margin));
        // Do not change activeWindow, virtual desktop, focus, or unrelated rules.
        window.frameGeometry = geometry;
    }

    function watch(window) {
        // Title sequence changes on each explicitly requested placement, which
        // also covers reused show/hide windows without following every resize.
        window.captionChanged.connect(function () { place(window); });
        window.minimizedChanged.connect(function () { place(window); });
        place(window);
    }
    workspace.windowAdded.connect(watch);
    workspace.windowList().forEach(watch);
}());
