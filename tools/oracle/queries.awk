# Prints oracle queries for one data set. Input: one line "umin umax vmin vmax" from the
# oracle's range command. Variable seed picks the random points.
#
# Queries: a lattice that includes the grid edges, points just outside positioned long
# sections, points beyond every border and corner, and seeded random points in a box that
# extends past the grid, then points several grid lengths away, where repeated and reflected
# borders wrap more than once. Each point gets z, xy, and pk.
#
# The generator is a Park-Miller LCG, so every awk produces the same numbers.

function rand01() {
    state = (state * 16807) % 2147483647
    return state / 2147483647
}

function point(u, v) {
    printf "z %.17g %.17g\n", u, v
    printf "xy %.17g %.17g\n", u, v
    printf "pk %.17g %.17g\n", u, v
}

{
    umin = $1; umax = $2; vmin = $3; vmax = $4
    state = seed
    ul = umax - umin; vl = vmax - vmin
    du = ul * 0.05; if (du < 1) du = 1
    dv = vl * 0.25; if (dv < 0.5) dv = 0.5
    umid = umin + ul / 2; vmid = vmin + vl / 2

    for (i = 0; i <= 11; i++)
        for (j = 0; j <= 6; j++)
            point(umin + ul * i / 11, vmin + vl * j / 6)

    point(umid, vmin - 5e-9); point(umid, vmax + 5e-9)
    point(umid, vmin - 2e-8); point(umid, vmax + 2e-8)
    point(umin - du, vmid); point(umax + du, vmid)
    point(umid, vmin - dv); point(umid, vmax + dv)
    point(umin - du, vmin - dv); point(umin - du, vmax + dv)
    point(umax + du, vmin - dv); point(umax + du, vmax + dv)
    point(umin - du, 0); point(umax + du, 0)

    for (i = 0; i < 150; i++)
        point(umin - du + rand01() * (ul + 2 * du), vmin - dv + rand01() * (vl + 2 * dv))

    for (i = 0; i < 30; i++)
        point(umin + (rand01() * 8 - 4) * ul, vmin + (rand01() * 8 - 4) * vl)
}
