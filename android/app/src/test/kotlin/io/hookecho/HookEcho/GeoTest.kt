package io.hookecho.HookEcho

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class GeoTest {
    /** A long, narrow warning, about 1.6 km wide and 40 km long, running north-south. */
    private val sliver = listOf(
        doubleArrayOf(35.10, -97.505),
        doubleArrayOf(35.10, -97.487),
        doubleArrayOf(35.46, -97.487),
        doubleArrayOf(35.46, -97.505),
    )

    @Test
    fun aNarrowWarningBetweenTheOldRimSamplesIsCaught() {
        // A place 11 mi east of the sliver's axis, at the latitude the sliver crosses, with a
        // 12 mi watch radius. The old six rim points (every 60°, from north) land at bearings 0,
        // 60, 120, 180, 240 and 300; those at 240 and 300 land 0.6 mi east of the axis, just
        // outside the sliver's 0.5 mi half-width, so the old sampling missed it. The circle
        // reaches it: the nearest edge is 10.5 mi away.
        val lat = 35.28
        val lon = -97.496 + 11 / (69.17 * Math.cos(Math.toRadians(lat)))
        assertTrue(Geo.circleTouches(lat, lon, 12.0, sliver))
        // With a 10 mi radius the circle stops short of it.
        assertFalse(Geo.circleTouches(lat, lon, 10.0, sliver))
    }

    @Test
    fun aPointInsideIsCoveredWithoutARadius() {
        assertTrue(Geo.circleTouches(35.30, -97.496, 0.0, sliver))
        assertFalse(Geo.circleTouches(35.30, -97.40, 0.0, sliver))
    }

    @Test
    fun aDrawnZoneOverlappingTheWarningIsCaught() {
        // A drawn zone straddling the sliver with no vertex inside it: only an edge crossing
        // shows the overlap.
        val zone = listOf(
            doubleArrayOf(35.25, -97.60),
            doubleArrayOf(35.25, -97.40),
            doubleArrayOf(35.30, -97.40),
            doubleArrayOf(35.30, -97.60),
        )
        assertTrue(Geo.polygonsTouch(zone, sliver))
        val elsewhere = zone.map { doubleArrayOf(it[0] + 1.0, it[1]) }
        assertFalse(Geo.polygonsTouch(elsewhere, sliver))
        // One polygon wholly inside the other.
        val inner = listOf(
            doubleArrayOf(35.20, -97.500),
            doubleArrayOf(35.20, -97.492),
            doubleArrayOf(35.22, -97.492),
        )
        assertTrue(Geo.polygonsTouch(inner, sliver))
        assertTrue(Geo.polygonsTouch(sliver, inner))
    }

    @Test
    fun degenerateInputIsNotAMatch() {
        assertFalse(Geo.circleTouches(35.3, -97.5, 50.0, sliver.take(2)))
        assertFalse(Geo.polygonsTouch(sliver, emptyList()))
    }
}
