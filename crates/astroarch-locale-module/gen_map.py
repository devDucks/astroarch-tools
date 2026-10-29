import json
import os

# Script to (re)generate the world map used by
# WorldMapView.qml, based on Natural Earth data (public
# domain). Run this again if you want to change the resolution (10m/50m/110m),
# the projection, or regenerate the map after a data update.
#
# Source used here: Admin 0 - Countries, scale 1:110m, retrieved
# from the official GitHub mirror:
#   https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_admin_0_countries.geojson
# (same data as naturalearthdata.com, just easier to fetch with curl
# than a .zip Shapefile—same license: public domain).
#
# Usage: python3 gen_map.py   (from the project root directory)

SRC = "map-source/ne_110m_admin_0_countries.geojson"
OUT_QML = "qml/WorldMapView.qml"
OUT_SVG = "assets/world_countries_preview.svg"
QML_MARKER_START = "    // === DONNÉES GÉNÉRÉES PAR gen_map.py -- DÉBUT (ne pas éditer à la main) ===\n"
QML_MARKER_END = "    // === DONNÉES GÉNÉRÉES PAR gen_map.py -- FIN ===\n"
W, H = 2000, 1000  # viewBox dimensions

def project(lon, lat):
    x = (lon + 180.0) / 360.0 * W
    y = (90.0 - lat) / 180.0 * H
    return round(x, 1), round(y, 1)

def ring_to_path(ring):
    pts = [project(lon, lat) for lon, lat in ring]
    if not pts:
        return ""
    d = f"M{pts[0][0]},{pts[0][1]}"
    for x, y in pts[1:]:
        d += f"L{x},{y}"
    d += "Z"
    return d

def geom_to_path(geom):
    t = geom["type"]
    coords = geom["coordinates"]
    parts = []
    if t == "Polygon":
        for ring in coords:
            parts.append(ring_to_path(ring))
    elif t == "MultiPolygon":
        for poly in coords:
            for ring in poly:
                parts.append(ring_to_path(ring))
    return "".join(parts)

def pick_code(p):
    for key in ("ISO_A2_EH", "ISO_A2", "WB_A2", "FIPS_10"):
        v = p.get(key)
        if v and v not in ("-99", ""):
            return v
    return ""

data = json.load(open(SRC))
entries = []
for f in data["features"]:
    p = f["properties"]
    if p.get("ISO_A3") == "ATA":  # Antarctica: Not covered in this module
        continue
    code = pick_code(p)
    name = p.get("NAME_EN") or p.get("NAME") or ""
    path = geom_to_path(f["geometry"])
    entries.append((code, name, path))

entries.sort(key=lambda e: e[1])

# --- Injects data directly into WorldMapView.qml -----------
# (cxx-qt-build 0.8's QmlModule::qml_file() rejects any file that does not
# end in .qml -- see the comment at the top of WorldMapView.qml
# -- so no separate .js file is possible here.)
with open(OUT_QML, encoding="utf-8") as f:
    qml_content = f.read()

block_lines = [QML_MARKER_START]
block_lines.append("    readonly property var worldCountries: [\n")
for code, name, path in entries:
    name_esc = name.replace('"', '\\"')
    block_lines.append('        { code: "%s", name: "%s", path: "%s" },\n' % (code, name_esc, path))
block_lines.append("    ]\n")
block_lines.append(QML_MARKER_END)
new_block = "".join(block_lines)

start_idx = qml_content.index(QML_MARKER_START)
end_idx = qml_content.index(QML_MARKER_END) + len(QML_MARKER_END)
qml_content = qml_content[:start_idx] + new_block + qml_content[end_idx:]

with open(OUT_QML, "w", encoding="utf-8") as f:
    f.write(qml_content)

print("entries:", len(entries))
print("WorldMapView.qml size:", os.path.getsize(OUT_QML), "bytes")

# --- Also generates a reference SVG (visual preview / debug) --------
with open(OUT_SVG, "w", encoding="utf-8") as out:
    out.write(f'<svg viewBox="0 0 {W} {H}" xmlns="http://www.w3.org/2000/svg">\n')
    out.write(f'<rect x="0" y="0" width="{W}" height="{H}" fill="#dbe9f4"/>\n')
    for code, name, path in entries:
        out.write(f'<path d="{path}" fill="#c9d6c9" stroke="#7a8f7a" stroke-width="0.5" fill-rule="evenodd"><title>{name}</title></path>\n')
    out.write('</svg>\n')

print("SVG preview size:", os.path.getsize(OUT_SVG), "bytes")
