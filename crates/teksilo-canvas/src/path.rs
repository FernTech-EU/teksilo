// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use crate::geometry::{Point, Rect, Transform2D};
use crate::paint::FillRule;
use teksilo_tokens::CornerRadius;

/// A path command for building arbitrary shapes (Tier 3 rendering).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    QuadTo {
        control: Point,
        to: Point,
    },
    CubicTo {
        control1: Point,
        control2: Point,
        to: Point,
    },
    ArcTo {
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
    },
    Close,
}

/// The seed a fresh [`Path`]'s [`stamp`](Path::stamp) starts from.
///
/// An arbitrary odd constant — the point is only that an empty path's stamp is
/// not `0`, so a stamp that was never folded is distinguishable from one folded
/// with a zero word.
const STAMP_SEED: u64 = 0x2545_F491_4F6C_DD1D;

/// Fold one 64-bit word into a rolling content stamp.
///
/// SplitMix64's finalizer, applied to `state ^ (word * golden)`. Chosen over a
/// plain FNV step because the words folded here are IEEE-754 bit patterns whose
/// high bits barely move between neighbouring coordinates, and FNV-1a's
/// avalanche is weakest exactly there — two points a texel apart would differ
/// in a handful of low bits and stay correlated through the fold.
///
/// Order-sensitive (the state is carried), so `MoveTo(a); LineTo(b)` and
/// `MoveTo(b); LineTo(a)` are distinct, and constant-time, which is the whole
/// reason this exists: see [`Path::stamp`].
#[inline]
const fn fold(state: u64, word: u64) -> u64 {
    let mut x = state ^ word.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[inline]
fn fold_f32(state: u64, v: f32) -> u64 {
    // `to_bits` rather than the value: `NaN` must fold reproducibly, and two
    // paths differing only in the sign of a zero are not the same path to a
    // rasterizer that winds by direction.
    fold(state, v.to_bits() as u64)
}

#[inline]
fn fold_point(state: u64, p: Point) -> u64 {
    fold_f32(fold_f32(state, p.x), p.y)
}

/// Fold one command into a rolling stamp.
fn fold_command(state: u64, cmd: &PathCommand) -> u64 {
    match *cmd {
        PathCommand::MoveTo(p) => fold_point(fold(state, 1), p),
        PathCommand::LineTo(p) => fold_point(fold(state, 2), p),
        PathCommand::QuadTo { control, to } => fold_point(fold_point(fold(state, 3), control), to),
        PathCommand::CubicTo {
            control1,
            control2,
            to,
        } => fold_point(
            fold_point(fold_point(fold(state, 4), control1), control2),
            to,
        ),
        PathCommand::ArcTo {
            rect,
            start_angle,
            sweep_angle,
        } => {
            let s = fold(state, 5);
            let s = fold_f32(
                fold_f32(fold_f32(fold_f32(s, rect.x), rect.y), rect.width),
                rect.height,
            );
            fold_f32(fold_f32(s, start_angle), sweep_angle)
        }
        PathCommand::Close => fold(state, 6),
    }
}

/// A path composed of drawing commands. Used for Tier 3 (CPU rasterized) shapes.
///
/// # Why the command list is not a public field
///
/// A path carries a **content stamp** ([`stamp`](Self::stamp)) maintained
/// incrementally as commands are appended, so a consumer that needs to know
/// "is this the same geometry I saw last frame?" can ask in constant time
/// instead of walking the commands. The renderer's path-mask cache is that
/// consumer, and the difference is not a micro-optimisation: keyed by a walk, a
/// *cache hit* on a long stroke cost microseconds and grew with the stroke — so
/// a stroke that grows by one point per pointer sample paid O(n) to discover it
/// had nothing to do, which is O(n²) over the stroke. The measured table lives
/// in `docs/ink.md` §5 and is quoted nowhere else, so that the two columns stay
/// one run of one machine; the gate that holds the shape rather than the
/// numbers is `a_cache_hit_does_not_scale_with_the_paths_length` in
/// `crates/teksilo-render/tests/wet_stroke_cost.rs`.
///
/// A public `Vec` cannot be kept in step with a stamp, so the commands are
/// reached through [`commands`](Self::commands) and appended through the
/// builders or [`push`](Self::push).
#[derive(Debug, Clone)]
pub struct Path {
    commands: Vec<PathCommand>,
    stamp: u64,
}

impl Default for Path {
    fn default() -> Self {
        Self {
            commands: Vec::new(),
            stamp: STAMP_SEED,
        }
    }
}

impl PartialEq for Path {
    /// Compares the commands. The stamp is a pure function of them, so
    /// including it would only be able to *agree*.
    fn eq(&self, other: &Self) -> bool {
        self.commands == other.commands
    }
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a path from a command list, folding each into the stamp.
    ///
    /// The door for a producer that assembles commands elsewhere — the SVG
    /// path-data parser is the one in-tree caller.
    pub fn from_commands(commands: Vec<PathCommand>) -> Self {
        let stamp = commands.iter().fold(STAMP_SEED, fold_command);
        Self { commands, stamp }
    }

    /// The commands, in order.
    #[inline]
    pub fn commands(&self) -> &[PathCommand] {
        &self.commands
    }

    /// A 64-bit stamp of this path's contents, maintained incrementally.
    ///
    /// Two paths with identical command sequences have identical stamps; two
    /// with different ones differ with the collision probability of a 64-bit
    /// hash. Reading it is O(1) — appending a command costs one fold, so
    /// building an n-command path costs O(n) *once* rather than O(n) per
    /// interrogation.
    ///
    /// Not stable across releases, and not to be persisted: it is an in-process
    /// identity for caches, nothing more.
    #[inline]
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// Append one command.
    pub fn push(&mut self, cmd: PathCommand) -> &mut Self {
        self.stamp = fold_command(self.stamp, &cmd);
        self.commands.push(cmd);
        self
    }

    /// Drop every command, returning to the empty path.
    pub fn clear(&mut self) -> &mut Self {
        self.commands.clear();
        self.stamp = STAMP_SEED;
        self
    }

    pub fn move_to(&mut self, p: Point) -> &mut Self {
        self.push(PathCommand::MoveTo(p))
    }

    pub fn line_to(&mut self, p: Point) -> &mut Self {
        self.push(PathCommand::LineTo(p))
    }

    pub fn quad_to(&mut self, control: Point, to: Point) -> &mut Self {
        self.push(PathCommand::QuadTo { control, to })
    }

    pub fn cubic_to(&mut self, control1: Point, control2: Point, to: Point) -> &mut Self {
        self.push(PathCommand::CubicTo {
            control1,
            control2,
            to,
        })
    }

    pub fn arc_to(&mut self, rect: Rect, start_angle: f32, sweep_angle: f32) -> &mut Self {
        self.push(PathCommand::ArcTo {
            rect,
            start_angle,
            sweep_angle,
        })
    }

    pub fn close(&mut self) -> &mut Self {
        self.push(PathCommand::Close)
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Compute the axis-aligned bounding box of this path.
    /// Only considers control points (not exact curve bounds), which is
    /// sufficient for atlas allocation.
    pub fn bounds(&self) -> Rect {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        let mut include = |p: Point| {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        };

        for cmd in &self.commands {
            match *cmd {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => include(p),
                PathCommand::QuadTo { control, to } => {
                    include(control);
                    include(to);
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    include(control1);
                    include(control2);
                    include(to);
                }
                PathCommand::ArcTo { rect, .. } => {
                    include(Point::new(rect.x, rect.y));
                    include(Point::new(rect.right(), rect.bottom()));
                }
                PathCommand::Close => {}
            }
        }

        if min_x > max_x {
            return Rect::ZERO;
        }
        Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    /// Create a circle path.
    pub fn circle(center: Point, radius: f32) -> Self {
        let rect = Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        );
        let mut path = Self::new();
        // A subpath must open with a `MoveTo`, or a renderer has no start point
        // for the arc and draws a stray line from the origin. The arc begins at
        // angle 0 — the rightmost point (center.x + radius, center.y).
        path.move_to(Point::new(center.x + radius, center.y));
        path.arc_to(rect, 0.0, 360.0);
        path.close();
        path
    }

    /// Create a rounded rectangle path using arc segments for each corner.
    pub fn rounded_rect(rect: Rect, radii: CornerRadius) -> Self {
        let [tl, tr, br, bl] = radii.to_array();
        let mut path = Self::new();

        // Start at top edge after top-left radius
        path.move_to(Point::new(rect.x + tl, rect.y));

        // Top edge → top-right arc
        path.line_to(Point::new(rect.right() - tr, rect.y));
        if tr > 0.0 {
            let arc_rect = Rect::new(rect.right() - tr * 2.0, rect.y, tr * 2.0, tr * 2.0);
            path.arc_to(arc_rect, -90.0, 90.0);
        }

        // Right edge → bottom-right arc
        path.line_to(Point::new(rect.right(), rect.bottom() - br));
        if br > 0.0 {
            let arc_rect = Rect::new(
                rect.right() - br * 2.0,
                rect.bottom() - br * 2.0,
                br * 2.0,
                br * 2.0,
            );
            path.arc_to(arc_rect, 0.0, 90.0);
        }

        // Bottom edge → bottom-left arc
        path.line_to(Point::new(rect.x + bl, rect.bottom()));
        if bl > 0.0 {
            let arc_rect = Rect::new(rect.x, rect.bottom() - bl * 2.0, bl * 2.0, bl * 2.0);
            path.arc_to(arc_rect, 90.0, 90.0);
        }

        // Left edge → top-left arc
        path.line_to(Point::new(rect.x, rect.y + tl));
        if tl > 0.0 {
            let arc_rect = Rect::new(rect.x, rect.y, tl * 2.0, tl * 2.0);
            path.arc_to(arc_rect, 180.0, 90.0);
        }

        path.close();
        path
    }

    /// Create a star path.
    pub fn star(center: Point, outer_radius: f32, inner_radius: f32, points: u32) -> Self {
        let mut path = Self::new();
        let total = points * 2;
        for i in 0..total {
            let angle =
                (i as f32) * std::f32::consts::PI / points as f32 - std::f32::consts::FRAC_PI_2;
            let r = if i % 2 == 0 {
                outer_radius
            } else {
                inner_radius
            };
            let p = Point::new(center.x + r * angle.cos(), center.y + r * angle.sin());
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        path.close();
        path
    }

    /// Create a non-rounded rectangle path.
    pub fn rect(rect: Rect) -> Self {
        let mut path = Self::new();
        path.move_to(Point::new(rect.x, rect.y));
        path.line_to(Point::new(rect.right(), rect.y));
        path.line_to(Point::new(rect.right(), rect.bottom()));
        path.line_to(Point::new(rect.x, rect.bottom()));
        path.close();
        path
    }

    /// Create a single line segment path.
    pub fn line(from: Point, to: Point) -> Self {
        let mut path = Self::new();
        path.move_to(from);
        path.line_to(to);
        path
    }

    /// Create an ellipse inscribed in the given rectangle (4 cubic Bézier arcs).
    pub fn ellipse(rect: Rect) -> Self {
        // Approximate an ellipse with 4 cubic Bézier curves.
        // Magic number for quarter-circle cubic approximation: κ ≈ 0.5522847498
        const KAPPA: f32 = 0.552_284_8;
        let cx = rect.x + rect.width / 2.0;
        let cy = rect.y + rect.height / 2.0;
        let rx = rect.width / 2.0;
        let ry = rect.height / 2.0;
        let kx = rx * KAPPA;
        let ky = ry * KAPPA;

        let mut path = Self::new();
        // Start at top center
        path.move_to(Point::new(cx, cy - ry));
        // Top-right quadrant
        path.cubic_to(
            Point::new(cx + kx, cy - ry),
            Point::new(cx + rx, cy - ky),
            Point::new(cx + rx, cy),
        );
        // Bottom-right quadrant
        path.cubic_to(
            Point::new(cx + rx, cy + ky),
            Point::new(cx + kx, cy + ry),
            Point::new(cx, cy + ry),
        );
        // Bottom-left quadrant
        path.cubic_to(
            Point::new(cx - kx, cy + ry),
            Point::new(cx - rx, cy + ky),
            Point::new(cx - rx, cy),
        );
        // Top-left quadrant
        path.cubic_to(
            Point::new(cx - rx, cy - ky),
            Point::new(cx - kx, cy - ry),
            Point::new(cx, cy - ry),
        );
        path.close();
        path
    }

    /// Append all commands from another path.
    pub fn append(&mut self, other: &Path) {
        for cmd in &other.commands {
            self.push(*cmd);
        }
    }

    /// Create a copy of this path with all points transformed by the given
    /// affine transform. **Exact for every affine**, curves included.
    ///
    /// Lines and Béziers transform by their points, which is exact by
    /// definition. An [`ArcTo`](PathCommand::ArcTo) cannot: it is stored as a
    /// rectangle plus two angles, and only a translate combined with a
    /// positive axis-aligned scale maps that onto another rectangle-plus-the-
    /// same-angles (see [`arc_transform_is_exact`]). Under a rotation, a shear
    /// or a mirror, transforming the rectangle and keeping the angles puts the
    /// arc's endpoints in the wrong place — a rotated rounded rectangle comes
    /// back with square-ish corners in the wrong quadrants. So an arc that the
    /// transform cannot carry is expanded into the cubics
    /// [`arc_to_cubics`] would have drawn it as, and those transform exactly.
    ///
    /// That expansion is the same approximation the renderer already paints
    /// the arc with, so a transformed path hit-tests as what is on screen. It
    /// is one-way: the returned path has cubics where this one had an arc.
    pub fn transformed(&self, transform: &Transform2D) -> Path {
        let arcs_survive = arc_transform_is_exact(transform);
        let mut result = Path::new();
        for cmd in &self.commands {
            match *cmd {
                PathCommand::MoveTo(p) => {
                    result.move_to(transform.apply_point(p));
                }
                PathCommand::LineTo(p) => {
                    result.line_to(transform.apply_point(p));
                }
                PathCommand::QuadTo { control, to } => {
                    result.quad_to(transform.apply_point(control), transform.apply_point(to));
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    result.cubic_to(
                        transform.apply_point(control1),
                        transform.apply_point(control2),
                        transform.apply_point(to),
                    );
                }
                PathCommand::ArcTo {
                    rect,
                    start_angle,
                    sweep_angle,
                } => {
                    if arcs_survive {
                        result.arc_to(transform.apply_rect(rect), start_angle, sweep_angle);
                    } else {
                        for seg in arc_to_cubics(rect, start_angle, sweep_angle) {
                            let from = transform.apply_point(seg.from);
                            match result.commands.last() {
                                // Already standing on the arc's start point:
                                // no connector.
                                Some(PathCommand::MoveTo(p) | PathCommand::LineTo(p))
                                    if *p == from => {}
                                Some(
                                    PathCommand::QuadTo { to, .. }
                                    | PathCommand::CubicTo { to, .. },
                                ) if *to == from => {}
                                // An arc that *opens* a subpath — the first
                                // command of the path, or the first after a
                                // `Close`. [`Path::flatten`] starts such a
                                // subpath at the arc's own first point, and
                                // that is what the `arcs_survive` branch above
                                // leaves the surviving `ArcTo` to do, so the
                                // expansion has to start one too. A `line_to`
                                // here would hand `flatten` a command with no
                                // subpath open, which opens one at the
                                // *cursor* — the point the `Close` returned it
                                // to, i.e. the previous subpath's start — and
                                // so joins two separate loops with an edge
                                // that is in neither the untransformed path
                                // nor the arc-preserving transform of it.
                                None | Some(PathCommand::Close) => {
                                    result.move_to(from);
                                }
                                // Continuing an open subpath from somewhere
                                // else: a straight connector, which is what
                                // `flatten` and the renderers both draw.
                                Some(_) => {
                                    result.line_to(from);
                                }
                            }
                            result.cubic_to(
                                transform.apply_point(seg.control1),
                                transform.apply_point(seg.control2),
                                transform.apply_point(seg.to),
                            );
                        }
                    }
                }
                PathCommand::Close => {
                    result.close();
                }
            }
        }
        result
    }

    /// Create a closed polygon from a list of points.
    pub fn polygon(points: &[Point]) -> Self {
        let mut path = Self::new();
        if let Some((&first, rest)) = points.split_first() {
            path.move_to(first);
            for &p in rest {
                path.line_to(p);
            }
            path.close();
        }
        path
    }

    /// Subdivide every curve command into line segments no further than
    /// `tolerance` from the true curve, yielding one [`Subpath`] per `MoveTo`
    /// run.
    ///
    /// `tolerance` is in the path's own units and is clamped to a sane floor,
    /// so a caller cannot ask for unbounded subdivision; each individual curve
    /// is additionally capped at 64 segments. Arcs go through
    /// [`arc_to_cubics`] first, so the whole command set is covered.
    ///
    /// A `LineTo` or curve arriving with no preceding `MoveTo` opens a subpath
    /// at the origin, matching how the renderers treat the same input.
    pub fn flatten(&self, tolerance: f32) -> Vec<Subpath> {
        let mut out: Vec<Subpath> = Vec::new();
        let mut current: Vec<Point> = Vec::new();
        let mut closed = false;
        let mut cursor = Point::ZERO;

        for cmd in &self.commands {
            match *cmd {
                PathCommand::MoveTo(p) => {
                    flush_subpath(&mut out, &mut current, &mut closed);
                    current.push(p);
                    cursor = p;
                }
                PathCommand::LineTo(p) => {
                    if current.is_empty() {
                        current.push(cursor);
                    }
                    current.push(p);
                    cursor = p;
                }
                PathCommand::QuadTo { control, to } => {
                    if current.is_empty() {
                        current.push(cursor);
                    }
                    push_quad(&mut current, cursor, control, to, tolerance);
                    cursor = to;
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    if current.is_empty() {
                        current.push(cursor);
                    }
                    push_cubic(&mut current, cursor, control1, control2, to, tolerance);
                    cursor = to;
                }
                PathCommand::ArcTo {
                    rect,
                    start_angle,
                    sweep_angle,
                } => {
                    for seg in arc_to_cubics(rect, start_angle, sweep_angle) {
                        if current.is_empty() {
                            current.push(seg.from);
                        } else if current[current.len() - 1] != seg.from {
                            // The arc starts away from the cursor: the renderers
                            // draw a straight connector there, so the polyline
                            // must carry the same edge.
                            current.push(seg.from);
                        }
                        push_cubic(
                            &mut current,
                            seg.from,
                            seg.control1,
                            seg.control2,
                            seg.to,
                            tolerance,
                        );
                        cursor = seg.to;
                    }
                }
                PathCommand::Close => {
                    if !current.is_empty() {
                        closed = true;
                        let start = current[0];
                        flush_subpath(&mut out, &mut current, &mut closed);
                        cursor = start;
                    }
                }
            }
        }
        flush_subpath(&mut out, &mut current, &mut closed);
        out
    }

    /// Curve-accurate bounding box, to within `tolerance`.
    ///
    /// [`Path::bounds`] is the *control-point hull* and over-reports every
    /// curve (a cubic never reaches its control points); this flattens first.
    /// Use `bounds()` where an over-estimate is free (atlas allocation) and
    /// this where the box is a hit-test or culling boundary.
    pub fn exact_bounds(&self, tolerance: f32) -> Rect {
        let subpaths = self.flatten(tolerance);
        let mut acc: Option<Rect> = None;
        for sp in &subpaths {
            if sp.points.is_empty() {
                continue;
            }
            let b = sp.bounds();
            acc = Some(match acc {
                None => b,
                Some(a) => {
                    let x = a.x.min(b.x);
                    let y = a.y.min(b.y);
                    Rect::new(
                        x,
                        y,
                        a.right().max(b.right()) - x,
                        a.bottom().max(b.bottom()) - y,
                    )
                }
            });
        }
        acc.unwrap_or(Rect::ZERO)
    }

    /// Whether `p` lies inside the path's filled region under `rule`.
    ///
    /// Every subpath is treated as closed, matching SVG fill semantics (an
    /// open subpath is filled as though a closing edge were present), so a
    /// ring authored as two subpaths has a real hole under
    /// [`FillRule::EvenOdd`].
    ///
    /// Flattens on every call. Callers testing the same path repeatedly
    /// should flatten once and reuse the [`Subpath`]s.
    pub fn contains_point(&self, p: Point, rule: FillRule, tolerance: f32) -> bool {
        subpaths_contain_point(&self.flatten(tolerance), p, rule)
    }
}

/// Close off the in-progress subpath, if it has any geometry, and reset the
/// closed flag for the next one.
fn flush_subpath(out: &mut Vec<Subpath>, current: &mut Vec<Point>, closed: &mut bool) {
    if !current.is_empty() {
        out.push(Subpath {
            points: std::mem::take(current),
            closed: *closed,
        });
    }
    *closed = false;
}

/// One cubic Bézier segment of an arc approximation.
///
/// `from` is the segment's start point — the previous segment's `to`, or the
/// arc's own start point for the first segment — so a consumer can emit each
/// segment independently without tracking a cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcCubic {
    pub from: Point,
    pub control1: Point,
    pub control2: Point,
    pub to: Point,
}

/// Whether `transform` maps an [`ArcTo`](PathCommand::ArcTo) — a rectangle plus
/// a start and sweep **angle** — onto another arc with the same angles.
///
/// It does exactly when the linear part is a positive axis-aligned scale (a
/// translation is always free). An arc point is
/// `(cx + rx·cos θ, cy + ry·sin θ)`; under `x ↦ a·x + tx`, `y ↦ d·y + ty` with
/// `a > 0` and `d > 0` that becomes the same parameterisation of the scaled
/// rectangle, so the rectangle can be transformed and the angles kept. A
/// rotation or shear mixes the two axes, and a negative scale mirrors the
/// sweep, so neither survives: [`Path::transformed`] expands the arc into
/// cubics instead.
pub fn arc_transform_is_exact(transform: &Transform2D) -> bool {
    const EPS: f32 = 1e-5;
    let [a, b, c, d, _, _] = transform.m;
    b.abs() < EPS && c.abs() < EPS && a > 0.0 && d > 0.0
}

/// Approximate an elliptical arc inscribed in `rect` with cubic Bézier
/// segments, one per 90° (or less) of sweep.
///
/// `start_angle` / `sweep_angle` are in **degrees**, matching
/// [`Path::arc_to`] and its callers ([`Path::circle`],
/// [`Path::rounded_rect`]). Returns an empty vector for a sweep small enough
/// to be invisible (below `0.001` rad ≈ 0.057°).
///
/// This is the one arc→cubic conversion in the tree: [`Path::flatten`] uses it,
/// [`Path::transformed`] uses it for an arc a transform cannot carry, and so
/// does `teksilo-render`'s path atlas (which scales each returned point into
/// device space before feeding tiny-skia).
pub fn arc_to_cubics(rect: Rect, start_angle: f32, sweep_angle: f32) -> Vec<ArcCubic> {
    let rx = rect.width * 0.5;
    let ry = rect.height * 0.5;
    let center_x = rect.x + rx;
    let center_y = rect.y + ry;

    let mut out = Vec::new();
    let mut remaining = sweep_angle.to_radians();
    let mut angle = start_angle.to_radians();
    if !remaining.is_finite() || !angle.is_finite() {
        return out;
    }
    let sign = if remaining >= 0.0 { 1.0 } else { -1.0 };

    while remaining.abs() > 0.001 {
        let chunk = sign * remaining.abs().min(std::f32::consts::FRAC_PI_2);
        let half = chunk * 0.5;
        let k = (4.0 / 3.0) * (1.0 - half.cos()) / half.sin();

        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let cos_b = (angle + chunk).cos();
        let sin_b = (angle + chunk).sin();

        out.push(ArcCubic {
            from: Point::new(center_x + rx * cos_a, center_y + ry * sin_a),
            control1: Point::new(
                center_x + rx * (cos_a - k * sin_a),
                center_y + ry * (sin_a + k * cos_a),
            ),
            control2: Point::new(
                center_x + rx * (cos_b + k * sin_b),
                center_y + ry * (sin_b - k * cos_b),
            ),
            to: Point::new(center_x + rx * cos_b, center_y + ry * sin_b),
        });

        angle += chunk;
        remaining -= chunk;
    }
    out
}

/// One flattened subpath: a polyline plus whether its author closed it.
///
/// Produced by [`Path::flatten`]. `points` always has the subpath's start
/// point first; the closing edge back to that point is **implied** by
/// `closed` rather than duplicated, so a triangle is three points, not four.
#[derive(Debug, Clone, PartialEq)]
pub struct Subpath {
    /// The polyline vertices, in order. Never empty for a subpath returned by
    /// [`Path::flatten`].
    pub points: Vec<Point>,
    /// Whether the author wrote a [`PathCommand::Close`] for this subpath.
    /// Fill treats every subpath as closed regardless (SVG semantics); a
    /// *stroke* only walks the closing edge when this is `true`.
    pub closed: bool,
}

impl Subpath {
    /// Axis-aligned bounding box of the polyline. [`Rect::ZERO`] when empty.
    ///
    /// This is what makes a per-subpath reject cheap: a point or region that
    /// misses this box cannot touch any of the subpath's segments, so a long
    /// ink stroke answers "no" without walking its segments.
    pub fn bounds(&self) -> Rect {
        bounds_of_points(&self.points)
    }

    /// Whether the polyline has no vertices.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Iterate the subpath's segments as `(from, to)` pairs, including the
    /// closing edge when `close_implicitly` is set (fill semantics) or when
    /// the subpath was explicitly closed.
    pub fn segments(&self, close_implicitly: bool) -> impl Iterator<Item = (Point, Point)> + '_ {
        let n = self.points.len();
        let wrap = (self.closed || close_implicitly) && n > 2;
        (0..n.saturating_sub(1))
            .map(move |i| (self.points[i], self.points[i + 1]))
            .chain(wrap.then(|| (self.points[n - 1], self.points[0])))
    }
}

fn bounds_of_points(points: &[Point]) -> Rect {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for p in points {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    if min_x > max_x {
        return Rect::ZERO;
    }
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Upper bound on the segments any one curve is subdivided into. A curve that
/// wants more than this is already far past the point where a hit-test can
/// tell the difference, and the cap is what keeps a pathological control
/// polygon from allocating without bound.
const MAX_CURVE_SEGMENTS: u32 = 64;

fn quad_segment_count(p0: Point, c: Point, p1: Point, tolerance: f32) -> u32 {
    // Max deviation of the n-segment polyline from a quadratic is
    // |p0 - 2c + p1| / (8 n²). Solve for n.
    let dx = p0.x - 2.0 * c.x + p1.x;
    let dy = p0.y - 2.0 * c.y + p1.y;
    let d = (dx * dx + dy * dy).sqrt();
    segment_count(d / 8.0, tolerance)
}

fn cubic_segment_count(p0: Point, c1: Point, c2: Point, p1: Point, tolerance: f32) -> u32 {
    // Max deviation of the n-segment polyline from a cubic is bounded by
    // (3/4) · max(|p0 - 2c1 + c2|, |c1 - 2c2 + p1|) / n².
    let d1 = ((p0.x - 2.0 * c1.x + c2.x).powi(2) + (p0.y - 2.0 * c1.y + c2.y).powi(2)).sqrt();
    let d2 = ((c1.x - 2.0 * c2.x + p1.x).powi(2) + (c1.y - 2.0 * c2.y + p1.y).powi(2)).sqrt();
    segment_count(0.75 * d1.max(d2), tolerance)
}

fn segment_count(numerator: f32, tolerance: f32) -> u32 {
    let tol = tolerance.max(1e-4);
    if !numerator.is_finite() || numerator <= 0.0 {
        return 1;
    }
    let n = (numerator / tol).sqrt().ceil();
    if !n.is_finite() {
        return MAX_CURVE_SEGMENTS;
    }
    (n as u32).clamp(1, MAX_CURVE_SEGMENTS)
}

fn push_quad(out: &mut Vec<Point>, p0: Point, c: Point, p1: Point, tolerance: f32) {
    let n = quad_segment_count(p0, c, p1, tolerance);
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let mt = 1.0 - t;
        out.push(Point::new(
            mt * mt * p0.x + 2.0 * mt * t * c.x + t * t * p1.x,
            mt * mt * p0.y + 2.0 * mt * t * c.y + t * t * p1.y,
        ));
    }
}

fn push_cubic(out: &mut Vec<Point>, p0: Point, c1: Point, c2: Point, p1: Point, tolerance: f32) {
    let n = cubic_segment_count(p0, c1, c2, p1, tolerance);
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let mt = 1.0 - t;
        let a = mt * mt * mt;
        let b = 3.0 * mt * mt * t;
        let c = 3.0 * mt * t * t;
        let d = t * t * t;
        out.push(Point::new(
            a * p0.x + b * c1.x + c * c2.x + d * p1.x,
            a * p0.y + b * c1.y + c * c2.y + d * p1.y,
        ));
    }
}

/// Point-in-polygons test over already-flattened subpaths.
///
/// Every subpath is treated as closed (SVG fill semantics). Shared by
/// [`Path::contains_point`] and by consumers that cache their flattening.
pub fn subpaths_contain_point(subpaths: &[Subpath], p: Point, rule: FillRule) -> bool {
    let mut winding: i32 = 0;
    let mut crossings: u32 = 0;
    for sp in subpaths {
        // Fewer than three vertices encloses no area. Counting such a
        // subpath's one edge would give a spurious winding: the closing edge
        // SVG semantics imply retraces it, and the two cancel.
        if sp.points.len() < 3 {
            continue;
        }
        for (a, b) in sp.segments(true) {
            // Half-open in y so a vertex shared by two edges is counted once.
            if (a.y <= p.y) == (b.y <= p.y) {
                continue;
            }
            let t = (p.y - a.y) / (b.y - a.y);
            let x = a.x + t * (b.x - a.x);
            if x > p.x {
                crossings += 1;
                winding += if b.y > a.y { 1 } else { -1 };
            }
        }
    }
    match rule {
        FillRule::Winding => winding != 0,
        FillRule::EvenOdd => crossings % 2 == 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Flattening, exact bounds, point-in-path
    // ------------------------------------------------------------------

    #[test]
    fn flatten_polyline_is_verbatim() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0))
            .line_to(Point::new(10.0, 10.0));
        let sp = path.flatten(0.25);
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].points.len(), 3);
        assert!(!sp[0].closed);
    }

    #[test]
    fn flatten_close_marks_subpath_and_does_not_duplicate_start() {
        let p = Path::rect(Rect::new(0.0, 0.0, 10.0, 10.0));
        let sp = p.flatten(0.25);
        assert_eq!(sp.len(), 1);
        assert!(sp[0].closed);
        // MoveTo + 3 LineTo = 4 vertices; the closing edge is implied.
        assert_eq!(sp[0].points.len(), 4);
    }

    #[test]
    fn flatten_splits_on_each_moveto() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0));
        path.move_to(Point::new(20.0, 20.0))
            .line_to(Point::new(30.0, 20.0));
        assert_eq!(path.flatten(0.25).len(), 2);
    }

    #[test]
    fn flatten_quad_tracks_the_true_curve() {
        // The quadratic from (0,0) with control (50,100) to (100,0) reaches
        // y = 50 at its apex, NOT the control point's y = 100.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
        let sp = path.flatten(0.25);
        let max_y = sp[0].points.iter().fold(f32::MIN, |m, p| m.max(p.y));
        assert!(
            (max_y - 50.0).abs() < 1.0,
            "apex should be ~50, got {max_y}"
        );
    }

    #[test]
    fn exact_bounds_beats_control_point_hull() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
        assert_eq!(path.bounds().height, 100.0, "hull over-reports");
        let exact = path.exact_bounds(0.25);
        assert!(
            (exact.height - 50.0).abs() < 1.0,
            "exact bounds should be ~50 tall, got {}",
            exact.height
        );
        assert!((exact.width - 100.0).abs() < 0.01);
    }

    #[test]
    fn exact_bounds_of_empty_path_is_zero() {
        assert_eq!(Path::new().exact_bounds(0.25), Rect::ZERO);
    }

    #[test]
    fn contains_point_square() {
        let p = Path::rect(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert!(p.contains_point(Point::new(50.0, 50.0), FillRule::Winding, 0.25));
        assert!(!p.contains_point(Point::new(150.0, 50.0), FillRule::Winding, 0.25));
        assert!(!p.contains_point(Point::new(50.0, -1.0), FillRule::Winding, 0.25));
    }

    #[test]
    fn contains_point_even_odd_ring_has_a_hole() {
        // Two concentric squares, same winding direction. Winding fills the
        // hole; even-odd punches it out.
        let mut p = Path::new();
        p.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 100.0))
            .line_to(Point::new(0.0, 100.0))
            .close();
        p.move_to(Point::new(25.0, 25.0))
            .line_to(Point::new(75.0, 25.0))
            .line_to(Point::new(75.0, 75.0))
            .line_to(Point::new(25.0, 75.0))
            .close();

        let centre = Point::new(50.0, 50.0);
        let band = Point::new(10.0, 50.0);
        assert!(p.contains_point(centre, FillRule::Winding, 0.25));
        assert!(!p.contains_point(centre, FillRule::EvenOdd, 0.25));
        assert!(p.contains_point(band, FillRule::Winding, 0.25));
        assert!(p.contains_point(band, FillRule::EvenOdd, 0.25));
    }

    #[test]
    fn contains_point_open_subpath_fills_as_closed() {
        // No `close()` - SVG fills it anyway.
        let mut p = Path::new();
        p.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 100.0))
            .line_to(Point::new(0.0, 100.0));
        assert!(p.contains_point(Point::new(50.0, 50.0), FillRule::Winding, 0.25));
    }

    #[test]
    fn contains_point_ignores_a_degenerate_subpath() {
        // A two-point "polygon" is a line: it encloses nothing, and a
        // half-plane answer here is what made a zero-width path query select
        // everything on one side of the line.
        let p = Path::line(Point::new(0.0, 0.0), Point::new(100.0, 100.0));
        assert!(!p.contains_point(Point::new(2.0, 85.0), FillRule::Winding, 0.25));
        assert!(!p.contains_point(Point::new(85.0, 2.0), FillRule::Winding, 0.25));
    }

    #[test]
    fn contains_point_circle() {
        let c = Path::circle(Point::new(50.0, 50.0), 25.0);
        assert!(c.contains_point(Point::new(50.0, 50.0), FillRule::Winding, 0.25));
        assert!(c.contains_point(Point::new(68.0, 50.0), FillRule::Winding, 0.25));
        assert!(!c.contains_point(Point::new(80.0, 50.0), FillRule::Winding, 0.25));
    }

    #[test]
    fn arc_to_cubics_covers_a_full_circle_in_four_segments() {
        let segs = arc_to_cubics(Rect::new(0.0, 0.0, 100.0, 100.0), 0.0, 360.0);
        assert_eq!(segs.len(), 4);
        // Starts at angle 0 - the rightmost point.
        assert!((segs[0].from.x - 100.0).abs() < 0.01);
        assert!((segs[0].from.y - 50.0).abs() < 0.01);
        // Each segment continues from the last.
        for w in segs.windows(2) {
            assert!((w[0].to.x - w[1].from.x).abs() < 0.01);
            assert!((w[0].to.y - w[1].from.y).abs() < 0.01);
        }
    }

    #[test]
    fn arc_to_cubics_ignores_a_sub_threshold_sweep() {
        assert!(arc_to_cubics(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0, 0.0).is_empty());
    }

    #[test]
    fn subpath_segments_wrap_only_when_closed() {
        let open = Subpath {
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
            ],
            closed: false,
        };
        assert_eq!(open.segments(false).count(), 2);
        assert_eq!(open.segments(true).count(), 3);
        let shut = Subpath {
            closed: true,
            ..open.clone()
        };
        assert_eq!(shut.segments(false).count(), 3);
    }

    #[test]
    fn subpath_bounds_is_the_polyline_box() {
        let sp = Subpath {
            points: vec![Point::new(10.0, 20.0), Point::new(50.0, 80.0)],
            closed: false,
        };
        assert_eq!(sp.bounds(), Rect::new(10.0, 20.0, 40.0, 60.0));
        assert_eq!(
            Subpath {
                points: vec![],
                closed: false
            }
            .bounds(),
            Rect::ZERO
        );
    }

    #[test]
    fn curve_subdivision_is_capped() {
        // A wildly-out-of-range control polygon must not allocate without
        // bound: one curve is capped at 64 segments (65 emitted points).
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0)).cubic_to(
            Point::new(1.0e9, 1.0e9),
            Point::new(-1.0e9, 1.0e9),
            Point::new(1.0, 0.0),
        );
        let sp = path.flatten(0.001);
        assert_eq!(sp[0].points.len(), 65);
    }

    #[test]
    fn flatten_tolerance_floor_keeps_a_zero_request_finite() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
        assert!(path.flatten(0.0)[0].points.len() <= 65);
    }

    #[test]
    fn circle_path_not_empty() {
        let p = Path::circle(Point::new(50.0, 50.0), 25.0);
        assert!(!p.is_empty());
    }

    #[test]
    fn circle_path_opens_with_moveto() {
        // A subpath must open with a MoveTo or a renderer draws a stray line
        // from the origin to the arc. The circle starts at angle 0 (rightmost).
        let p = Path::circle(Point::new(50.0, 50.0), 25.0);
        match p.commands.first() {
            Some(PathCommand::MoveTo(pt)) => {
                assert!((pt.x - 75.0).abs() < 0.01 && (pt.y - 50.0).abs() < 0.01);
            }
            other => panic!("circle must open with MoveTo, got {other:?}"),
        }
    }

    #[test]
    fn star_path_has_correct_commands() {
        let p = Path::star(Point::new(50.0, 50.0), 30.0, 15.0, 5);
        assert!(!p.is_empty());
        // 5-point star: 10 vertices + 1 close
        assert_eq!(p.commands.len(), 11);
    }

    #[test]
    fn bounds_of_simple_path() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 20.0));
        path.line_to(Point::new(50.0, 80.0));
        path.line_to(Point::new(30.0, 40.0));
        let b = path.bounds();
        assert_eq!(b.x, 10.0);
        assert_eq!(b.y, 20.0);
        assert_eq!(b.width, 40.0);
        assert_eq!(b.height, 60.0);
    }

    #[test]
    fn bounds_of_empty_path_is_zero() {
        let path = Path::new();
        let b = path.bounds();
        assert_eq!(b, Rect::ZERO);
    }

    #[test]
    fn rounded_rect_uses_corner_radii() {
        use teksilo_tokens::CornerRadius;
        let p = Path::rounded_rect(
            Rect::new(0.0, 0.0, 100.0, 50.0),
            CornerRadius::uniform(10.0),
        );
        // Should have arcs for each corner — more commands than a plain rect
        assert!(p.commands.len() > 5);
        // Should contain ArcTo commands
        let arc_count = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::ArcTo { .. }))
            .count();
        assert_eq!(arc_count, 4); // One per corner
    }

    #[test]
    fn rounded_rect_zero_radii_is_plain_rect() {
        use teksilo_tokens::CornerRadius;
        let p = Path::rounded_rect(Rect::new(0.0, 0.0, 100.0, 50.0), CornerRadius::uniform(0.0));
        // No arcs with zero radii
        let arc_count = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::ArcTo { .. }))
            .count();
        assert_eq!(arc_count, 0);
    }

    #[test]
    fn rect_path_has_four_lines() {
        let p = Path::rect(Rect::new(10.0, 20.0, 30.0, 40.0));
        // MoveTo + 3 LineTo + Close = 5 commands
        assert_eq!(p.commands.len(), 5);
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(p.commands[4], PathCommand::Close));
    }

    #[test]
    fn line_path() {
        let p = Path::line(Point::new(0.0, 0.0), Point::new(100.0, 50.0));
        assert_eq!(p.commands.len(), 2);
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(p.commands[1], PathCommand::LineTo(_)));
    }

    #[test]
    fn ellipse_path_uses_cubics() {
        let p = Path::ellipse(Rect::new(0.0, 0.0, 100.0, 50.0));
        let cubic_count = p
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::CubicTo { .. }))
            .count();
        assert_eq!(cubic_count, 4);
    }

    #[test]
    fn polygon_path() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(50.0, 80.0),
        ];
        let p = Path::polygon(&points);
        // MoveTo + 2 LineTo + Close = 4 commands
        assert_eq!(p.commands.len(), 4);
    }

    #[test]
    fn polygon_empty_points() {
        let p = Path::polygon(&[]);
        assert!(p.is_empty());
    }

    #[test]
    fn append_merges_paths() {
        let mut a = Path::new();
        a.move_to(Point::new(0.0, 0.0));
        a.line_to(Point::new(10.0, 10.0));

        let mut b = Path::new();
        b.move_to(Point::new(20.0, 20.0));
        b.line_to(Point::new(30.0, 30.0));

        a.append(&b);
        assert_eq!(a.commands.len(), 4);
        assert!(matches!(a.commands[2], PathCommand::MoveTo(p) if (p.x - 20.0).abs() < 0.01));
    }

    #[test]
    fn append_empty_is_noop() {
        let mut a = Path::new();
        a.move_to(Point::new(1.0, 2.0));
        let b = Path::new();
        a.append(&b);
        assert_eq!(a.commands.len(), 1);
    }

    #[test]
    fn transformed_translate() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 20.0));
        path.line_to(Point::new(30.0, 40.0));
        path.close();

        let t = Transform2D::translate(100.0, 200.0);
        let result = path.transformed(&t);

        assert_eq!(result.commands.len(), 3);
        match result.commands[0] {
            PathCommand::MoveTo(p) => {
                assert!((p.x - 110.0).abs() < 0.01);
                assert!((p.y - 220.0).abs() < 0.01);
            }
            _ => panic!("expected MoveTo"),
        }
        match result.commands[1] {
            PathCommand::LineTo(p) => {
                assert!((p.x - 130.0).abs() < 0.01);
                assert!((p.y - 240.0).abs() < 0.01);
            }
            _ => panic!("expected LineTo"),
        }
        assert!(matches!(result.commands[2], PathCommand::Close));
    }

    #[test]
    fn transformed_scale() {
        let mut path = Path::new();
        path.move_to(Point::new(10.0, 20.0));
        path.quad_to(Point::new(15.0, 25.0), Point::new(30.0, 40.0));

        let t = Transform2D::scale(2.0, 3.0);
        let result = path.transformed(&t);

        match result.commands[1] {
            PathCommand::QuadTo { control, to } => {
                assert!((control.x - 30.0).abs() < 0.01);
                assert!((control.y - 75.0).abs() < 0.01);
                assert!((to.x - 60.0).abs() < 0.01);
                assert!((to.y - 120.0).abs() < 0.01);
            }
            _ => panic!("expected QuadTo"),
        }
    }

    #[test]
    fn transformed_cubic() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.cubic_to(
            Point::new(1.0, 0.0),
            Point::new(2.0, 1.0),
            Point::new(3.0, 3.0),
        );

        let t = Transform2D::translate(10.0, 20.0);
        let result = path.transformed(&t);

        match result.commands[1] {
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                assert!((control1.x - 11.0).abs() < 0.01);
                assert!((control2.x - 12.0).abs() < 0.01);
                assert!((to.x - 13.0).abs() < 0.01);
            }
            _ => panic!("expected CubicTo"),
        }
    }

    #[test]
    fn transformed_preserves_command_count() {
        let p = Path::circle(Point::new(50.0, 50.0), 25.0);
        let t = Transform2D::scale(2.0, 2.0);
        let result = p.transformed(&t);
        assert_eq!(result.commands.len(), p.commands.len());
    }

    #[test]
    fn arc_transform_is_exact_only_for_a_positive_axis_aligned_scale() {
        assert!(arc_transform_is_exact(&Transform2D::identity()));
        assert!(arc_transform_is_exact(&Transform2D::translate(9.0, -3.0)));
        assert!(arc_transform_is_exact(&Transform2D::scale(2.0, 7.0)));
        assert!(
            !arc_transform_is_exact(&Transform2D::rotate(0.4)),
            "a rotation mixes the axes"
        );
        assert!(
            !arc_transform_is_exact(&Transform2D::rotate(std::f32::consts::FRAC_PI_2)),
            "even a quarter turn: the angles would have to move with it"
        );
        assert!(
            !arc_transform_is_exact(&Transform2D::scale(-1.0, 1.0)),
            "a mirror reverses the sweep"
        );
    }

    #[test]
    fn a_rotated_circle_is_still_that_circle() {
        // The defect this exists for: transforming an `ArcTo`'s *rectangle* and
        // keeping its angles turns a rotation into a resize. A circle of radius
        // 30 about the origin is invariant under rotation, so any growth is the
        // bug, and a point 38 out must stay outside at every angle.
        let c = Path::circle(Point::ZERO, 30.0);
        for turns in 1..8 {
            let t = Transform2D::rotate(turns as f32 * 0.4);
            let rotated = c.transformed(&t);
            let b = rotated.exact_bounds(0.25);
            assert!(
                (b.width - 60.0).abs() < 0.5 && (b.height - 60.0).abs() < 0.5,
                "rotation by {turns} resized the circle to {b:?}"
            );
            let outside = t.apply_point(Point::new(38.0, 0.0));
            assert!(
                !rotated.contains_point(outside, FillRule::Winding, 0.25),
                "a pure rotation must not change containment (turn {turns})"
            );
            assert!(rotated.contains_point(
                t.apply_point(Point::new(20.0, 0.0)),
                FillRule::Winding,
                0.25
            ));
        }
    }

    #[test]
    fn a_rotated_rounded_rect_keeps_its_corners_where_they_belong() {
        let r = Path::rounded_rect(
            Rect::new(-50.0, -50.0, 100.0, 100.0),
            teksilo_tokens::CornerRadius::uniform(25.0),
        );
        let t = Transform2D::rotate(std::f32::consts::FRAC_PI_4);
        let rotated = r.transformed(&t);
        assert!(rotated.contains_point(Point::ZERO, FillRule::Winding, 0.25));
        // The rotated silhouette is the same rounded square turned half a right
        // angle, so the image of a point in the original's transparent corner
        // is still out.
        let corner = t.apply_point(Point::new(-49.0, -49.0));
        assert!(!rotated.contains_point(corner, FillRule::Winding, 0.25));
        // ...and the image of a point on the original's edge is still in.
        let edge = t.apply_point(Point::new(0.0, -49.0));
        assert!(rotated.contains_point(edge, FillRule::Winding, 0.25));
    }

    #[test]
    fn an_arc_the_transform_cannot_carry_becomes_cubics_not_a_moved_rectangle() {
        // The mechanism, stated: the arc survives as an arc under a scale and
        // is expanded under a rotation. Deleting the expansion makes the
        // rotated path keep its `ArcTo` and this reddens.
        let c = Path::circle(Point::ZERO, 10.0);
        let arcs = |p: &Path| {
            p.commands
                .iter()
                .filter(|c| matches!(c, PathCommand::ArcTo { .. }))
                .count()
        };
        assert!(arcs(&c) > 0);
        assert_eq!(
            arcs(&c.transformed(&Transform2D::scale(3.0, 2.0))),
            arcs(&c)
        );
        assert_eq!(arcs(&c.transformed(&Transform2D::rotate(0.3))), 0);
    }

    // ------------------------------------------------------------------
    // `transformed` preserves subpath structure — the two branches, checked
    // against one another
    // ------------------------------------------------------------------

    /// The tolerance every structural comparison below flattens at.
    const STRUCT_TOL: f32 = 0.05;

    /// One flattened subpath, described in the **source** path's own frame:
    /// whether the author closed it, where it starts, and the ground it
    /// covers.
    ///
    /// Deliberately says nothing about how many vertices the subpath has or
    /// where they fall: the two branches of [`Path::transformed`] sample a
    /// curve at different places by construction (one flattens a surviving
    /// arc, the other flattens the cubics it was expanded into), and the
    /// arc→cubic expansion is free to change. What may never differ is which
    /// loops there are and where each one runs.
    #[derive(Debug, Clone, Copy)]
    struct SubpathShape {
        closed: bool,
        start: Point,
        bounds: Rect,
    }

    fn structure(p: &Path, back: &Transform2D) -> Vec<SubpathShape> {
        p.flatten(STRUCT_TOL)
            .iter()
            .map(|sp| {
                let pts: Vec<Point> = sp.points.iter().map(|q| back.apply_point(*q)).collect();
                SubpathShape {
                    closed: sp.closed,
                    start: pts[0],
                    bounds: bounds_of_points(&pts),
                }
            })
            .collect()
    }

    /// Transforming a path and flattening it must give the same loops as
    /// flattening it and transforming the points — for **every** affine, so
    /// the arc-preserving branch and the arc-expanding branch are held to one
    /// rule and therefore to each other.
    ///
    /// Bounds are compared within a few times the flatten tolerance because
    /// the two branches genuinely sample a curve at different parameters; a
    /// structural defect is off by the distance between two subpaths, not by
    /// a fraction of the flattening error.
    #[track_caller]
    fn assert_transform_commutes_with_flatten(label: &str, p: &Path, t: &Transform2D) {
        const NEAR: f32 = 4.0 * STRUCT_TOL;
        let back = t.inverse().expect("test transforms are invertible");
        let plain = structure(p, &Transform2D::identity());
        let moved = structure(&p.transformed(t), &back);
        assert_eq!(
            moved.len(),
            plain.len(),
            "{label}: {} subpaths after the transform, {} before\n  after:  {moved:?}\n  before: {plain:?}",
            moved.len(),
            plain.len()
        );
        for (i, (m, e)) in moved.iter().zip(&plain).enumerate() {
            assert_eq!(m.closed, e.closed, "{label}: subpath {i} closed flag");
            assert!(
                (m.start.x - e.start.x).abs() < NEAR && (m.start.y - e.start.y).abs() < NEAR,
                "{label}: subpath {i} starts at {:?}, not {:?}",
                m.start,
                e.start
            );
            assert!(
                (m.bounds.x - e.bounds.x).abs() < NEAR
                    && (m.bounds.y - e.bounds.y).abs() < NEAR
                    && (m.bounds.width - e.bounds.width).abs() < NEAR
                    && (m.bounds.height - e.bounds.height).abs() < NEAR,
                "{label}: subpath {i} covers {:?}, not {:?}",
                m.bounds,
                e.bounds
            );
        }
    }

    /// One arc rect, used by the battery below.
    fn arc_rect() -> Rect {
        Rect::new(180.0, -20.0, 40.0, 40.0)
    }

    fn square() -> Path {
        let mut p = Path::new();
        p.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0))
            .line_to(Point::new(10.0, 10.0))
            .line_to(Point::new(0.0, 10.0))
            .close();
        p
    }

    /// Every position an `ArcTo` — and, for contrast, a `QuadTo` / `CubicTo` —
    /// can sit in relative to a `Close` and to each other.
    fn structural_battery() -> Vec<(&'static str, Path)> {
        let mut out: Vec<(&'static str, Path)> = Vec::new();

        // The defect this section exists for: a closed loop, then an arc.
        let mut p = square();
        p.arc_to(arc_rect(), 0.0, 360.0).close();
        out.push(("close then full-sweep arc", p));

        // ...and with a partial sweep, where the spurious edge encloses real
        // area instead of exactly retracing the closing edge.
        let mut p = square();
        p.arc_to(arc_rect(), 0.0, 200.0).close();
        out.push(("close then partial arc", p));

        let mut p = Path::new();
        p.arc_to(arc_rect(), 0.0, 360.0);
        out.push(("arc opens the path", p));

        let mut p = Path::new();
        p.close();
        p.arc_to(arc_rect(), 0.0, 360.0);
        out.push(("close with no geometry, then arc", p));

        let mut p = Path::new();
        p.move_to(Point::new(5.0, 5.0)).close();
        p.arc_to(arc_rect(), 0.0, 360.0);
        out.push(("lone move_to, close, then arc", p));

        let mut p = Path::new();
        p.move_to(Point::new(220.0, 0.0));
        p.arc_to(arc_rect(), 0.0, 180.0);
        p.arc_to(arc_rect(), 180.0, 180.0);
        out.push(("arc continuing an arc at the shared point", p));

        let mut p = Path::new();
        p.arc_to(arc_rect(), 0.0, 90.0);
        p.arc_to(Rect::new(0.0, 0.0, 20.0, 20.0), 0.0, 90.0);
        out.push(("arc after a disjoint arc", p));

        let mut p = Path::new();
        p.move_to(Point::new(50.0, 50.0));
        p.arc_to(arc_rect(), 0.0, 360.0);
        out.push(("arc after a move_to somewhere else", p));

        let mut p = square();
        p.quad_to(Point::new(120.0, -40.0), Point::new(200.0, 0.0));
        out.push(("close then quad", p));

        let mut p = square();
        p.cubic_to(
            Point::new(60.0, -40.0),
            Point::new(140.0, 40.0),
            Point::new(200.0, 0.0),
        );
        out.push(("close then cubic", p));

        let mut p = Path::new();
        p.quad_to(Point::new(120.0, -40.0), Point::new(200.0, 0.0));
        out.push(("quad opens the path", p));

        let mut p = Path::new();
        p.cubic_to(
            Point::new(60.0, -40.0),
            Point::new(140.0, 40.0),
            Point::new(200.0, 0.0),
        );
        out.push(("cubic opens the path", p));

        let mut p = square();
        p.line_to(Point::new(200.0, 0.0));
        out.push(("close then line", p));

        out.push(("rounded rect", {
            Path::rounded_rect(
                Rect::new(-50.0, -50.0, 100.0, 100.0),
                teksilo_tokens::CornerRadius::uniform(25.0),
            )
        }));
        out.push(("circle", Path::circle(Point::new(30.0, 10.0), 20.0)));

        out
    }

    #[test]
    fn transformed_preserves_subpath_structure_in_both_branches() {
        // A positive axis-aligned scale keeps the `ArcTo` (the `arcs_survive`
        // branch); a rotation expands it (the other one). Holding both to the
        // same oracle is what makes this a comparison *between the branches*
        // rather than a fixture of whatever the expansion currently emits.
        let survives = Transform2D::scale(3.0, 2.0);
        let expands = Transform2D::rotate(30f32.to_radians());
        assert!(arc_transform_is_exact(&survives));
        assert!(!arc_transform_is_exact(&expands));

        for (label, path) in structural_battery() {
            assert_transform_commutes_with_flatten(
                &format!("{label} / arcs survive"),
                &path,
                &survives,
            );
            assert_transform_commutes_with_flatten(
                &format!("{label} / arcs expand"),
                &path,
                &expands,
            );
        }
    }

    #[test]
    fn an_expanded_arc_after_a_close_opens_its_own_subpath() {
        // The defect, named: the expansion emitted a `line_to`, which
        // `flatten` opens at the cursor the `Close` left behind — the
        // *previous* loop's start — welding two separate loops together.
        let mut p = square();
        p.arc_to(arc_rect(), 0.0, 360.0).close();

        let rotated = p.transformed(&Transform2D::rotate(30f32.to_radians()));
        let after_close = rotated
            .commands
            .iter()
            .position(|c| matches!(c, PathCommand::Close))
            .expect("the square is closed")
            + 1;
        assert!(
            matches!(rotated.commands[after_close], PathCommand::MoveTo(_)),
            "the arc has to open a subpath, got {:?}",
            rotated.commands[after_close]
        );

        // And the loop it opens still covers only the arc: 40 x 40, not the
        // 220-wide box a connector back to the square's start would give it.
        let sub = rotated.flatten(STRUCT_TOL);
        assert_eq!(sub.len(), 2, "two loops, still");
        let back = Transform2D::rotate(30f32.to_radians()).inverse().unwrap();
        let pts: Vec<Point> = sub[1].points.iter().map(|q| back.apply_point(*q)).collect();
        let b = bounds_of_points(&pts);
        assert!(
            (b.width - 40.0).abs() < 0.2 && (b.height - 40.0).abs() < 0.2,
            "the arc's loop grew to {b:?}"
        );
    }

    #[test]
    fn a_rotated_arc_that_opens_a_subpath_still_opens_one() {
        // An `arc_to` with no preceding `move_to`: the expansion has to start
        // the subpath itself rather than emitting a dangling `line_to`.
        let mut p = Path::new();
        p.arc_to(Rect::new(0.0, 0.0, 20.0, 20.0), 0.0, 360.0);
        let rotated = p.transformed(&Transform2D::rotate(0.7));
        assert!(matches!(
            rotated.commands.first(),
            Some(PathCommand::MoveTo(_))
        ));
        assert_eq!(rotated.flatten(0.25).len(), 1);
    }
}
