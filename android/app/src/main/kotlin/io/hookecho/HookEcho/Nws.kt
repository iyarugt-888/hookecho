package io.hookecho.HookEcho

import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/**
 * The watched-points alert lookup, shared by the background service and the home-screen widget.
 *
 * Kotlin-only and stateless on purpose: `api.weather.gov/alerts/active?point=` answers "what is
 * warned here" with no geometry work, and the watch list is read from the same `settings.json` the
 * Rust app writes — a shared file rather than an IPC channel, since neither caller can assume the
 * app is running.
 */
object Nws {
    const val TIER_WATCH = 1
    const val TIER_WARNING = 2
    const val TIER_EMERGENCY = 3
    private const val USER_AGENT = "hookecho (github.com/d4vid87/hookecho)"

    /**
     * A watched place: [lat]/[lon] is where a notification flies the camera, [samples] is every
     * point asked about. A marker with a watch radius contributes its rim as well as its centre,
     * and a drawn zone contributes its vertices, so a warning that stops down the road is still
     * a warning here.
     */
    data class Watch(
        val name: String,
        val lat: Double,
        val lon: Double,
        val samples: List<DoubleArray>,
        /** A saved place's watch radius; 0 for a drawn zone or a place without one. */
        val radiusMi: Double = 0.0,
        /** A drawn zone's ring, `[lat, lon]`; empty for a saved place. */
        val ring: List<DoubleArray> = emptyList(),
    )

    data class Alert(val id: String, val event: String, val headline: String, val tier: Int)

    /** An alert that carries its own polygon (outer rings, `[lat, lon]` vertices). */
    data class PolyAlert(val alert: Alert, val rings: List<List<DoubleArray>>)

    /**
     * Is the phone inside the user's quiet-hours window? Same `settings.json` fields, and the
     * same reading as the desktop: start == end is no window rather than all day, and a window
     * whose end is before its start wraps midnight.
     *
     * The escalated tier is not filtered here — callers let it through regardless, which is the
     * point of the tier.
     */
    fun inQuietHours(filesDir: File, hour: Int): Boolean {
        val f = File(filesDir, "config/settings.json")
        if (!f.exists()) return false
        val root = runCatching { JSONObject(f.readText()) }.getOrNull() ?: return false
        if (!root.optBoolean("quiet_hours", false)) return false
        val s = Math.floorMod(root.optInt("quiet_start_hour", 22), 24)
        val e = Math.floorMod(root.optInt("quiet_end_hour", 7), 24)
        val h = Math.floorMod(hour, 24)
        return when {
            s == e -> false
            s < e -> h in s until e
            else -> h >= s || h < e
        }
    }

    /** Rim points around a marker, so `?point=` sees a warning that only reaches the radius. */
    private const val RIM_POINTS = 6

    /** Vertices sampled per drawn zone, evenly spaced around its ring. */
    private const val ZONE_POINTS = 8

    /** Ceiling on `?point=` requests per pass; api.weather.gov is not ours to hammer. */
    private const val SAMPLE_CAP = 40

    /**
     * Saved markers and drawn zones from settings.json — the same file the Rust app reads and
     * writes (`markers[].name/lat/lon/alert_radius_mi`, `alert_polygons[].name/ring`).
     *
     * These samples are for `?point=`, which resolves zone-only alerts (null geometry) on the
     * server. Alerts that carry a polygon are also tested against each place's whole watch circle
     * or zone ([polygonAlerts], [covers]), so a narrow warning between two samples is still
     * caught (1010.md V2).
     */
    fun watched(filesDir: File): List<Watch> {
        val f = File(filesDir, "config/settings.json")
        if (!f.exists()) return emptyList()
        val root = JSONObject(f.readText())
        val out = ArrayList<Watch>()

        val markers = root.optJSONArray("markers")
        for (i in 0 until (markers?.length() ?: 0)) {
            val m = markers?.optJSONObject(i) ?: continue
            val lat = m.optDouble("lat")
            val lon = m.optDouble("lon")
            if (!lat.isFinite() || !lon.isFinite()) continue
            val radiusMi = m.optDouble("alert_radius_mi", 0.0).let { if (it.isFinite()) it else 0.0 }
            val samples = ArrayList<DoubleArray>()
            samples.add(doubleArrayOf(lat, lon))
            if (radiusMi > 0.0) {
                for (k in 0 until RIM_POINTS) samples.add(offset(lat, lon, radiusMi, k * 360.0 / RIM_POINTS))
            }
            out.add(Watch(m.optString("name", "Saved location"), lat, lon, samples, radiusMi = radiusMi))
        }

        val zones = root.optJSONArray("alert_polygons")
        for (i in 0 until (zones?.length() ?: 0)) {
            val z = zones?.optJSONObject(i) ?: continue
            val ring = z.optJSONArray("ring") ?: continue
            val pts = (0 until ring.length()).mapNotNull { k ->
                val p = ring.optJSONArray(k) ?: return@mapNotNull null
                val lon = p.optDouble(0)
                val lat = p.optDouble(1)
                if (lat.isFinite() && lon.isFinite()) doubleArrayOf(lat, lon) else null
            }
            if (pts.isEmpty()) continue
            val lat = pts.sumOf { it[0] } / pts.size
            val lon = pts.sumOf { it[1] } / pts.size
            val step = maxOf(1, pts.size / ZONE_POINTS)
            val samples = ArrayList<DoubleArray>()
            samples.add(doubleArrayOf(lat, lon))
            for (k in pts.indices step step) samples.add(pts[k])
            out.add(Watch(z.optString("name", "Watch zone"), lat, lon, samples, ring = pts))
        }

        // Trim from the back so early markers keep their rim rather than every place losing it.
        var budget = SAMPLE_CAP
        return out.map { w ->
            val take = w.samples.take(maxOf(1, minOf(w.samples.size, budget)))
            budget -= take.size
            w.copy(samples = take)
        }
    }

    /** Destination point `distMi` from (`lat`,`lon`) on `bearingDeg`, spherical earth. */
    private fun offset(lat: Double, lon: Double, distMi: Double, bearingDeg: Double): DoubleArray {
        val ang = distMi / 3958.8
        val br = Math.toRadians(bearingDeg)
        val la = Math.toRadians(lat)
        val lo = Math.toRadians(lon)
        val la2 = Math.asin(Math.sin(la) * Math.cos(ang) + Math.cos(la) * Math.sin(ang) * Math.cos(br))
        val lo2 = lo + Math.atan2(
            Math.sin(br) * Math.sin(ang) * Math.cos(la),
            Math.cos(ang) - Math.sin(la) * Math.sin(la2),
        )
        return doubleArrayOf(Math.toDegrees(la2), Math.toDegrees(lo2))
    }

    /**
     * Whether [w] touches a warning polygon [ring]: a drawn zone by overlap, a saved place by its
     * watch circle (or the point itself without a radius).
     */
    fun covers(w: Watch, ring: List<DoubleArray>): Boolean =
        if (w.ring.size >= 3) {
            Geo.polygonsTouch(w.ring, ring)
        } else {
            Geo.circleTouches(w.lat, w.lon, w.radiusMi, ring)
        }

    /**
     * The state a watched place is in (`OK`), from `api.weather.gov/points`, cached by place in
     * `alert_states.txt` so it is asked once. `null` offshore, outside the U.S., or offline.
     */
    fun stateFor(filesDir: File, lat: Double, lon: Double): String? {
        val key = "%.2f,%.2f".format(java.util.Locale.US, lat, lon)
        val cache = File(filesDir, "alert_states.txt")
        val known = runCatching {
            cache.readLines().mapNotNull { l -> l.split('\t').takeIf { it.size == 2 } }
                .associate { it[0] to it[1] }
        }.getOrDefault(emptyMap())
        known[key]?.let { return it }
        val body = get("https://api.weather.gov/points/%.4f,%.4f".format(java.util.Locale.US, lat, lon)) ?: return null
        val state = runCatching {
            JSONObject(body).getJSONObject("properties").getJSONObject("relativeLocation")
                .getJSONObject("properties").getString("state")
        }.getOrNull()?.takeIf { it.length == 2 } ?: return null
        runCatching { cache.appendText("$key\t$state\n") }
        return state
    }

    /**
     * Every active alert in [states] that carries a polygon, with its rings (1010.md V2). One
     * request covers every watched place. The `?point=` samples stay for alerts that only name
     * zones (null geometry), which the server resolves and this geometry cannot.
     */
    fun polygonAlerts(states: Collection<String>): List<PolyAlert> {
        if (states.isEmpty()) return emptyList()
        val body = get("https://api.weather.gov/alerts/active?area=" + states.sorted().joinToString(","))
            ?: return emptyList()
        val features = JSONObject(body).optJSONArray("features") ?: return emptyList()
        return (0 until features.length()).mapNotNull { i ->
            val f = features.optJSONObject(i) ?: return@mapNotNull null
            val p = f.optJSONObject("properties") ?: return@mapNotNull null
            val rings = ringsOf(f.optJSONObject("geometry"))
            if (rings.isEmpty()) return@mapNotNull null
            val event = p.optString("event")
            val id = p.optString("id").ifEmpty { event + p.optString("sent") }
            val alert = Alert(id, event, p.optString("headline", event), tierOf(p, event))
            PolyAlert(alert, rings).takeIf { alert.tier > 0 }
        }
    }

    /** The outer rings of a GeoJSON Polygon or MultiPolygon, as `[lat, lon]` vertices. */
    private fun ringsOf(geometry: JSONObject?): List<List<DoubleArray>> {
        if (geometry == null) return emptyList()
        val coords = geometry.optJSONArray("coordinates") ?: return emptyList()
        val polygons = when (geometry.optString("type")) {
            "Polygon" -> listOf(coords)
            "MultiPolygon" -> (0 until coords.length()).mapNotNull { coords.optJSONArray(it) }
            else -> return emptyList()
        }
        return polygons.mapNotNull { poly ->
            val outer = poly.optJSONArray(0) ?: return@mapNotNull null
            (0 until outer.length()).mapNotNull { k ->
                val pt = outer.optJSONArray(k) ?: return@mapNotNull null
                val lon = pt.optDouble(0)
                val lat = pt.optDouble(1)
                if (lat.isFinite() && lon.isFinite()) doubleArrayOf(lat, lon) else null
            }.takeIf { it.size >= 3 }
        }
    }

    /** A GET to api.weather.gov; `null` on any non-200 answer. */
    private fun get(url: String): String? {
        val conn = (URL(url).openConnection() as HttpURLConnection).apply {
            setRequestProperty("User-Agent", USER_AGENT)
            setRequestProperty("Accept", "application/geo+json")
            connectTimeout = 15_000
            readTimeout = 15_000
        }
        return try {
            if (conn.responseCode != 200) null else conn.inputStream.bufferedReader().readText()
        } finally {
            conn.disconnect()
        }
    }

    fun alertsAt(lat: Double, lon: Double): List<Alert> {
        val url = URL("https://api.weather.gov/alerts/active?point=%.4f,%.4f".format(java.util.Locale.US, lat, lon))
        val conn = (url.openConnection() as HttpURLConnection).apply {
            setRequestProperty("User-Agent", USER_AGENT)
            setRequestProperty("Accept", "application/geo+json")
            connectTimeout = 15_000
            readTimeout = 15_000
        }
        val body = try {
            if (conn.responseCode != 200) return emptyList()
            conn.inputStream.bufferedReader().readText()
        } finally {
            conn.disconnect()
        }
        val features = JSONObject(body).optJSONArray("features") ?: return emptyList()
        return (0 until features.length()).mapNotNull { i ->
            val p = features.optJSONObject(i)?.optJSONObject("properties") ?: return@mapNotNull null
            val event = p.optString("event")
            val id = p.optString("id").ifEmpty { event + p.optString("sent") }
            Alert(id, event, p.optString("headline", event), tierOf(p, event))
        }.filter { it.tier > 0 }
    }

    /**
     * Escalation tier from the alert's own words. `tornadoDamageThreat` is the field that
     * separates a routine tornado warning from a PDS/emergency one, and it is only ever set on
     * the alerts that matter.
     */
    private fun tierOf(p: JSONObject, event: String): Int {
        val threat = p.optJSONObject("parameters")?.optJSONArray("tornadoDamageThreat")
            ?.optString(0).orEmpty()
        val text = (p.optString("description") + " " + p.optString("headline")).uppercase()
        return when {
            threat == "CATASTROPHIC" || text.contains("TORNADO EMERGENCY") -> TIER_EMERGENCY
            threat == "CONSIDERABLE" || event.contains("Flash Flood Emergency") -> TIER_EMERGENCY
            event.endsWith("Warning") -> TIER_WARNING
            event.endsWith("Watch") -> TIER_WATCH
            else -> 0
        }
    }
}
