package io.hookecho.HookEcho

import kotlin.math.cos
import kotlin.math.hypot

/**
 * Whether a watched place touches a warning polygon, for background alerts (1010.md V2).
 *
 * Plain arithmetic on `[lat, lon]` arrays, with no Android types, so it is unit-tested on the JVM
 * (`GeoTest`). Points are projected onto a local plane in kilometres around a reference point:
 * equirectangular, which is well inside a warning polygon's size for any error that matters here.
 */
object Geo {
    private const val KM_PER_MI = 1.609344

    /**
     * Whether a circle of [radiusMi] around ([lat], [lon]) touches the polygon [ring]. Its
     * vertices are `[lat, lon]`, and it may be closed or open. A zero radius is a plain point in
     * polygon test.
     */
    fun circleTouches(lat: Double, lon: Double, radiusMi: Double, ring: List<DoubleArray>): Boolean {
        if (ring.size < 3) return false
        val pts = ring.map { project(lat, lon, it) }
        if (inside(0.0, 0.0, pts)) return true
        val r = radiusMi.coerceAtLeast(0.0) * KM_PER_MI
        if (r == 0.0) return false
        return pts.indices.any { i ->
            val a = pts[i]
            val b = pts[(i + 1) % pts.size]
            pointToSegment(0.0, 0.0, a, b) <= r
        }
    }

    /** Whether two polygons, each `[lat, lon]` vertices, overlap or touch. */
    fun polygonsTouch(a: List<DoubleArray>, b: List<DoubleArray>): Boolean {
        if (a.size < 3 || b.size < 3) return false
        val (lat0, lon0) = a[0][0] to a[0][1]
        val pa = a.map { project(lat0, lon0, it) }
        val pb = b.map { project(lat0, lon0, it) }
        if (pa.any { inside(it[0], it[1], pb) } || pb.any { inside(it[0], it[1], pa) }) return true
        for (i in pa.indices) {
            val a1 = pa[i]
            val a2 = pa[(i + 1) % pa.size]
            for (j in pb.indices) {
                if (segmentsCross(a1, a2, pb[j], pb[(j + 1) % pb.size])) return true
            }
        }
        return false
    }

    /** `[lat, lon]` to kilometres east and north of ([lat0], [lon0]). */
    private fun project(lat0: Double, lon0: Double, p: DoubleArray): DoubleArray =
        doubleArrayOf(
            (p[1] - lon0) * 111.32 * cos(Math.toRadians(lat0)),
            (p[0] - lat0) * 110.57,
        )

    /** Ray casting; a point on an edge may land either way, which the edge tests cover. */
    private fun inside(x: Double, y: Double, poly: List<DoubleArray>): Boolean {
        var c = false
        var j = poly.size - 1
        for (i in poly.indices) {
            val (xi, yi) = poly[i][0] to poly[i][1]
            val (xj, yj) = poly[j][0] to poly[j][1]
            if ((yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi) c = !c
            j = i
        }
        return c
    }

    private fun pointToSegment(x: Double, y: Double, a: DoubleArray, b: DoubleArray): Double {
        val dx = b[0] - a[0]
        val dy = b[1] - a[1]
        val len2 = dx * dx + dy * dy
        val t = if (len2 == 0.0) 0.0 else (((x - a[0]) * dx + (y - a[1]) * dy) / len2).coerceIn(0.0, 1.0)
        return hypot(x - (a[0] + t * dx), y - (a[1] + t * dy))
    }

    private fun cross(o: DoubleArray, a: DoubleArray, b: DoubleArray): Double =
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

    private fun segmentsCross(p1: DoubleArray, p2: DoubleArray, q1: DoubleArray, q2: DoubleArray): Boolean {
        val d1 = cross(q1, q2, p1)
        val d2 = cross(q1, q2, p2)
        val d3 = cross(p1, p2, q1)
        val d4 = cross(p1, p2, q2)
        return (d1 > 0) != (d2 > 0) && (d3 > 0) != (d4 > 0)
    }
}
