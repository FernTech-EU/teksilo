//! SVG path `d` attribute parser.
//!
//! Converts an SVG path data string into a sequence of [`PathCommand`]s.
//! Supports all 20 SVG path commands (10 absolute + 10 relative):
//! M/m, L/l, H/h, V/v, C/c, S/s, Q/q, T/t, A/a, Z/z.
//!
//! SVG arcs (A/a) are converted to cubic Bézier sequences using the
//! endpoint-to-center conversion from SVG spec §F.6.5.

use crate::geometry::Point;
use crate::path::PathCommand;

use super::SvgParseError;

/// Parse an SVG path data string into fern `PathCommand`s.
pub(crate) fn parse_svg_path_data(d: &str) -> Result<Vec<PathCommand>, SvgParseError> {
    let mut parser = PathDataParser::new(d);
    parser.parse()?;
    Ok(parser.commands)
}

struct PathDataParser<'a> {
    input: &'a [u8],
    pos: usize,
    commands: Vec<PathCommand>,
    // Current pen position
    cx: f32,
    cy: f32,
    // Sub-path start (for Z)
    sx: f32,
    sy: f32,
    // Previous control point (for S/s and T/t reflection)
    prev_control: Option<Point>,
    // Previous command type (for reflection logic)
    prev_cmd: Option<u8>,
}

impl<'a> PathDataParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
            commands: Vec::new(),
            cx: 0.0,
            cy: 0.0,
            sx: 0.0,
            sy: 0.0,
            prev_control: None,
            prev_cmd: None,
        }
    }

    fn parse(&mut self) -> Result<(), SvgParseError> {
        self.skip_wsp();
        while self.pos < self.input.len() {
            let cmd = self.input[self.pos];
            if cmd.is_ascii_alphabetic() {
                self.pos += 1;
                self.skip_wsp_comma();
                self.parse_command(cmd)?;
            } else {
                // Implicit repeat of previous command
                let Some(prev) = self.prev_cmd else {
                    return Err(self.error("expected command letter"));
                };
                // Per SVG spec: implicit repeat of M becomes L, m becomes l
                let implicit = match prev {
                    b'M' => b'L',
                    b'm' => b'l',
                    other => other,
                };
                self.parse_command(implicit)?;
            }
            self.skip_wsp();
        }
        Ok(())
    }

    fn parse_command(&mut self, cmd: u8) -> Result<(), SvgParseError> {
        match cmd {
            b'M' | b'm' => self.parse_moveto(cmd == b'm'),
            b'L' | b'l' => self.parse_lineto(cmd == b'l'),
            b'H' | b'h' => self.parse_horizontal(cmd == b'h'),
            b'V' | b'v' => self.parse_vertical(cmd == b'v'),
            b'C' | b'c' => self.parse_cubic(cmd == b'c'),
            b'S' | b's' => self.parse_smooth_cubic(cmd == b's'),
            b'Q' | b'q' => self.parse_quad(cmd == b'q'),
            b'T' | b't' => self.parse_smooth_quad(cmd == b't'),
            b'A' | b'a' => self.parse_arc(cmd == b'a'),
            b'Z' | b'z' => self.parse_close(cmd),
            _ => Err(self.error(&format!("unknown command '{}'", cmd as char))),
        }
    }

    fn parse_moveto(&mut self, relative: bool) -> Result<(), SvgParseError> {
        let x = self.read_number()?;
        let y = self.read_number()?;
        let (ax, ay) = if relative {
            (self.cx + x, self.cy + y)
        } else {
            (x, y)
        };
        self.cx = ax;
        self.cy = ay;
        self.sx = ax;
        self.sy = ay;
        self.commands.push(PathCommand::MoveTo(Point::new(ax, ay)));
        self.prev_control = None;
        self.prev_cmd = Some(if relative { b'm' } else { b'M' });

        // Additional coordinate pairs become implicit LineTo
        self.skip_wsp_comma();
        while self.has_number() {
            let x = self.read_number()?;
            let y = self.read_number()?;
            let (ax, ay) = if relative {
                (self.cx + x, self.cy + y)
            } else {
                (x, y)
            };
            self.cx = ax;
            self.cy = ay;
            self.commands.push(PathCommand::LineTo(Point::new(ax, ay)));
            self.prev_control = None;
            self.prev_cmd = Some(if relative { b'l' } else { b'L' });
            self.skip_wsp_comma();
        }
        Ok(())
    }

    fn parse_lineto(&mut self, relative: bool) -> Result<(), SvgParseError> {
        loop {
            let x = self.read_number()?;
            let y = self.read_number()?;
            let (ax, ay) = if relative {
                (self.cx + x, self.cy + y)
            } else {
                (x, y)
            };
            self.cx = ax;
            self.cy = ay;
            self.commands.push(PathCommand::LineTo(Point::new(ax, ay)));
            self.prev_control = None;
            self.prev_cmd = Some(if relative { b'l' } else { b'L' });
            self.skip_wsp_comma();
            if !self.has_number() {
                break;
            }
        }
        Ok(())
    }

    fn parse_horizontal(&mut self, relative: bool) -> Result<(), SvgParseError> {
        loop {
            let x = self.read_number()?;
            let ax = if relative { self.cx + x } else { x };
            self.cx = ax;
            self.commands
                .push(PathCommand::LineTo(Point::new(ax, self.cy)));
            self.prev_control = None;
            self.prev_cmd = Some(if relative { b'h' } else { b'H' });
            self.skip_wsp_comma();
            if !self.has_number() {
                break;
            }
        }
        Ok(())
    }

    fn parse_vertical(&mut self, relative: bool) -> Result<(), SvgParseError> {
        loop {
            let y = self.read_number()?;
            let ay = if relative { self.cy + y } else { y };
            self.cy = ay;
            self.commands
                .push(PathCommand::LineTo(Point::new(self.cx, ay)));
            self.prev_control = None;
            self.prev_cmd = Some(if relative { b'v' } else { b'V' });
            self.skip_wsp_comma();
            if !self.has_number() {
                break;
            }
        }
        Ok(())
    }

    fn parse_cubic(&mut self, relative: bool) -> Result<(), SvgParseError> {
        loop {
            let x1 = self.read_number()?;
            let y1 = self.read_number()?;
            let x2 = self.read_number()?;
            let y2 = self.read_number()?;
            let x = self.read_number()?;
            let y = self.read_number()?;
            let (c1, c2, to) = if relative {
                (
                    Point::new(self.cx + x1, self.cy + y1),
                    Point::new(self.cx + x2, self.cy + y2),
                    Point::new(self.cx + x, self.cy + y),
                )
            } else {
                (Point::new(x1, y1), Point::new(x2, y2), Point::new(x, y))
            };
            self.commands.push(PathCommand::CubicTo {
                control1: c1,
                control2: c2,
                to,
            });
            self.prev_control = Some(c2);
            self.cx = to.x;
            self.cy = to.y;
            self.prev_cmd = Some(if relative { b'c' } else { b'C' });
            self.skip_wsp_comma();
            if !self.has_number() {
                break;
            }
        }
        Ok(())
    }

    fn parse_smooth_cubic(&mut self, relative: bool) -> Result<(), SvgParseError> {
        loop {
            let x2 = self.read_number()?;
            let y2 = self.read_number()?;
            let x = self.read_number()?;
            let y = self.read_number()?;
            // Reflect previous control2 for control1
            let c1 = match (self.prev_cmd, self.prev_control) {
                (Some(b'C' | b'c' | b'S' | b's'), Some(pc)) => {
                    Point::new(2.0 * self.cx - pc.x, 2.0 * self.cy - pc.y)
                }
                _ => Point::new(self.cx, self.cy),
            };
            let (c2, to) = if relative {
                (
                    Point::new(self.cx + x2, self.cy + y2),
                    Point::new(self.cx + x, self.cy + y),
                )
            } else {
                (Point::new(x2, y2), Point::new(x, y))
            };
            self.commands.push(PathCommand::CubicTo {
                control1: c1,
                control2: c2,
                to,
            });
            self.prev_control = Some(c2);
            self.cx = to.x;
            self.cy = to.y;
            self.prev_cmd = Some(if relative { b's' } else { b'S' });
            self.skip_wsp_comma();
            if !self.has_number() {
                break;
            }
        }
        Ok(())
    }

    fn parse_quad(&mut self, relative: bool) -> Result<(), SvgParseError> {
        loop {
            let x1 = self.read_number()?;
            let y1 = self.read_number()?;
            let x = self.read_number()?;
            let y = self.read_number()?;
            let (ctrl, to) = if relative {
                (
                    Point::new(self.cx + x1, self.cy + y1),
                    Point::new(self.cx + x, self.cy + y),
                )
            } else {
                (Point::new(x1, y1), Point::new(x, y))
            };
            self.commands
                .push(PathCommand::QuadTo { control: ctrl, to });
            self.prev_control = Some(ctrl);
            self.cx = to.x;
            self.cy = to.y;
            self.prev_cmd = Some(if relative { b'q' } else { b'Q' });
            self.skip_wsp_comma();
            if !self.has_number() {
                break;
            }
        }
        Ok(())
    }

    fn parse_smooth_quad(&mut self, relative: bool) -> Result<(), SvgParseError> {
        loop {
            let x = self.read_number()?;
            let y = self.read_number()?;
            // Reflect previous control
            let ctrl = match (self.prev_cmd, self.prev_control) {
                (Some(b'Q' | b'q' | b'T' | b't'), Some(pc)) => {
                    Point::new(2.0 * self.cx - pc.x, 2.0 * self.cy - pc.y)
                }
                _ => Point::new(self.cx, self.cy),
            };
            let to = if relative {
                Point::new(self.cx + x, self.cy + y)
            } else {
                Point::new(x, y)
            };
            self.commands
                .push(PathCommand::QuadTo { control: ctrl, to });
            self.prev_control = Some(ctrl);
            self.cx = to.x;
            self.cy = to.y;
            self.prev_cmd = Some(if relative { b't' } else { b'T' });
            self.skip_wsp_comma();
            if !self.has_number() {
                break;
            }
        }
        Ok(())
    }

    fn parse_arc(&mut self, relative: bool) -> Result<(), SvgParseError> {
        loop {
            let rx = self.read_number()?.abs();
            let ry = self.read_number()?.abs();
            let x_rotation = self.read_number()?;
            let large_arc = self.read_flag()?;
            let sweep = self.read_flag()?;
            let x = self.read_number()?;
            let y = self.read_number()?;
            let (ax, ay) = if relative {
                (self.cx + x, self.cy + y)
            } else {
                (x, y)
            };
            // Convert endpoint arc to cubic Bézier(s)
            arc_to_cubics(
                self.cx,
                self.cy,
                rx,
                ry,
                x_rotation,
                large_arc,
                sweep,
                ax,
                ay,
                &mut self.commands,
            );
            self.cx = ax;
            self.cy = ay;
            self.prev_control = None;
            self.prev_cmd = Some(if relative { b'a' } else { b'A' });
            self.skip_wsp_comma();
            if !self.has_number() {
                break;
            }
        }
        Ok(())
    }

    fn parse_close(&mut self, cmd: u8) -> Result<(), SvgParseError> {
        self.commands.push(PathCommand::Close);
        self.cx = self.sx;
        self.cy = self.sy;
        self.prev_control = None;
        self.prev_cmd = Some(cmd);
        Ok(())
    }

    // --- Tokenizer ---

    fn skip_wsp(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn skip_wsp_comma(&mut self) {
        self.skip_wsp();
        if self.pos < self.input.len() && self.input[self.pos] == b',' {
            self.pos += 1;
            self.skip_wsp();
        }
    }

    fn has_number(&self) -> bool {
        if self.pos >= self.input.len() {
            return false;
        }
        let c = self.input[self.pos];
        c.is_ascii_digit() || c == b'.' || c == b'-' || c == b'+'
    }

    fn read_number(&mut self) -> Result<f32, SvgParseError> {
        self.skip_wsp_comma();
        let start = self.pos;
        if self.pos < self.input.len()
            && (self.input[self.pos] == b'-' || self.input[self.pos] == b'+')
        {
            self.pos += 1;
        }
        let mut has_digits = false;
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
            has_digits = true;
        }
        if self.pos < self.input.len() && self.input[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
                has_digits = true;
            }
        }
        // Scientific notation
        if self.pos < self.input.len()
            && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E')
        {
            self.pos += 1;
            if self.pos < self.input.len()
                && (self.input[self.pos] == b'-' || self.input[self.pos] == b'+')
            {
                self.pos += 1;
            }
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if !has_digits || start == self.pos {
            return Err(self.error("expected number"));
        }
        let s = std::str::from_utf8(&self.input[start..self.pos])
            .expect("number tokens consist of ASCII digits/sign/dot/exponent");
        s.parse::<f32>()
            .map_err(|_| self.error(&format!("invalid number '{s}'")))
    }

    fn read_flag(&mut self) -> Result<bool, SvgParseError> {
        self.skip_wsp_comma();
        if self.pos >= self.input.len() {
            return Err(self.error("expected flag (0 or 1)"));
        }
        match self.input[self.pos] {
            b'0' => {
                self.pos += 1;
                Ok(false)
            }
            b'1' => {
                self.pos += 1;
                Ok(true)
            }
            _ => Err(self.error("expected flag (0 or 1)")),
        }
    }

    fn error(&self, detail: &str) -> SvgParseError {
        SvgParseError::InvalidPathData {
            detail: detail.to_string(),
            position: self.pos,
        }
    }
}

// --- SVG arc endpoint → cubic Bézier conversion (SVG spec §F.6) ---

#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    x1: f32,
    y1: f32,
    mut rx: f32,
    mut ry: f32,
    x_rotation_deg: f32,
    large_arc: bool,
    sweep: bool,
    x2: f32,
    y2: f32,
    out: &mut Vec<PathCommand>,
) {
    // Degenerate: endpoints coincide
    if (x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6 {
        return;
    }
    // Degenerate: zero radius → line
    if rx < 1e-6 || ry < 1e-6 {
        out.push(PathCommand::LineTo(Point::new(x2, y2)));
        return;
    }

    let phi = x_rotation_deg.to_radians();
    let cos_phi = phi.cos();
    let sin_phi = phi.sin();

    // Step 1: Compute (x1', y1') — rotated midpoint
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;

    // Step 2: Ensure radii are large enough (F.6.6.3)
    let x1p2 = x1p * x1p;
    let y1p2 = y1p * y1p;
    let mut rx2 = rx * rx;
    let mut ry2 = ry * ry;
    let lambda = x1p2 / rx2 + y1p2 / ry2;
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
        rx2 = rx * rx;
        ry2 = ry * ry;
    }

    // Step 3: Compute center point (cx', cy')
    let num = (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2).max(0.0);
    let den = rx2 * y1p2 + ry2 * x1p2;
    let sq = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let cxp = sign * sq * (rx * y1p / ry);
    let cyp = sign * sq * -(ry * x1p / rx);

    // Step 4: Compute center in original coordinates
    let mx = (x1 + x2) / 2.0;
    let my = (y1 + y2) / 2.0;
    let _cx = cos_phi * cxp - sin_phi * cyp + mx;
    let _cy = sin_phi * cxp + cos_phi * cyp + my;

    // Step 5: Compute start angle and sweep angle
    let theta1 = angle_between(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dtheta = angle_between(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );

    if !sweep && dtheta > 0.0 {
        dtheta -= std::f32::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f32::consts::TAU;
    }

    // Step 6: Split into <=90° segments and approximate with cubics
    let n_segs = (dtheta.abs() / std::f32::consts::FRAC_PI_2).ceil() as usize;
    let n_segs = n_segs.max(1);
    let seg_angle = dtheta / n_segs as f32;

    let mut theta = theta1;
    let mut px = x1;
    let mut py = y1;

    for _ in 0..n_segs {
        let next_theta = theta + seg_angle;
        let (c1x, c1y, c2x, c2y, ex, ey) =
            arc_segment_to_cubic(rx, ry, cos_phi, sin_phi, _cx, _cy, theta, seg_angle);
        // Use computed endpoint for last segment to avoid drift
        let end_x = if theta + seg_angle == theta1 + dtheta {
            x2
        } else {
            ex
        };
        let end_y = if theta + seg_angle == theta1 + dtheta {
            y2
        } else {
            ey
        };
        let _ = (px, py); // silence unused warning
        out.push(PathCommand::CubicTo {
            control1: Point::new(c1x, c1y),
            control2: Point::new(c2x, c2y),
            to: Point::new(end_x, end_y),
        });
        px = end_x;
        py = end_y;
        theta = next_theta;
    }
}

/// Approximate a single arc segment (<=90°) with a cubic Bézier.
/// Returns (c1x, c1y, c2x, c2y, ex, ey) in original coordinates.
#[allow(clippy::too_many_arguments)]
fn arc_segment_to_cubic(
    rx: f32,
    ry: f32,
    cos_phi: f32,
    sin_phi: f32,
    cx: f32,
    cy: f32,
    theta: f32,
    d_theta: f32,
) -> (f32, f32, f32, f32, f32, f32) {
    let alpha =
        (d_theta / 2.0).sin() * ((4.0 + 3.0 * (d_theta / 2.0).tan().powi(2)).sqrt() - 1.0) / 3.0;

    let cos1 = theta.cos();
    let sin1 = theta.sin();
    let cos2 = (theta + d_theta).cos();
    let sin2 = (theta + d_theta).sin();

    // Endpoint on unit ellipse (pre-rotation)
    let ep1x = rx * cos1;
    let ep1y = ry * sin1;
    let ep2x = rx * cos2;
    let ep2y = ry * sin2;

    // Derivatives
    let d1x = -rx * sin1;
    let d1y = ry * cos1;
    let d2x = -rx * sin2;
    let d2y = ry * cos2;

    // Control points on unit ellipse
    let q1x = ep1x + alpha * d1x;
    let q1y = ep1y + alpha * d1y;
    let q2x = ep2x - alpha * d2x;
    let q2y = ep2y - alpha * d2y;

    // Transform to original coordinate space
    let c1x = cos_phi * q1x - sin_phi * q1y + cx;
    let c1y = sin_phi * q1x + cos_phi * q1y + cy;
    let c2x = cos_phi * q2x - sin_phi * q2y + cx;
    let c2y = sin_phi * q2x + cos_phi * q2y + cy;
    let ex = cos_phi * ep2x - sin_phi * ep2y + cx;
    let ey = sin_phi * ep2x + cos_phi * ep2y + cy;

    (c1x, c1y, c2x, c2y, ex, ey)
}

/// Angle between vectors (ux, uy) and (vx, vy) in radians.
fn angle_between(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    let dot = ux * vx + uy * vy;
    let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
    let cos_val = (dot / len).clamp(-1.0, 1.0);
    let angle = cos_val.acos();
    if ux * vy - uy * vx < 0.0 {
        -angle
    } else {
        angle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_point(p: Point, x: f32, y: f32) {
        assert!(
            (p.x - x).abs() < 0.01 && (p.y - y).abs() < 0.01,
            "expected ({x}, {y}), got ({}, {})",
            p.x,
            p.y
        );
    }

    #[test]
    fn moveto_lineto() {
        let cmds = parse_svg_path_data("M 10 20 L 30 40").unwrap();
        assert_eq!(cmds.len(), 2);
        match cmds[0] {
            PathCommand::MoveTo(p) => assert_point(p, 10.0, 20.0),
            _ => panic!("expected MoveTo"),
        }
        match cmds[1] {
            PathCommand::LineTo(p) => assert_point(p, 30.0, 40.0),
            _ => panic!("expected LineTo"),
        }
    }

    #[test]
    fn relative_lineto() {
        let cmds = parse_svg_path_data("M 10 20 l 5 10").unwrap();
        assert_eq!(cmds.len(), 2);
        match cmds[1] {
            PathCommand::LineTo(p) => assert_point(p, 15.0, 30.0),
            _ => panic!("expected LineTo"),
        }
    }

    #[test]
    fn horizontal_vertical() {
        let cmds = parse_svg_path_data("M 0 0 H 50 V 30").unwrap();
        assert_eq!(cmds.len(), 3);
        match cmds[1] {
            PathCommand::LineTo(p) => assert_point(p, 50.0, 0.0),
            _ => panic!("expected LineTo for H"),
        }
        match cmds[2] {
            PathCommand::LineTo(p) => assert_point(p, 50.0, 30.0),
            _ => panic!("expected LineTo for V"),
        }
    }

    #[test]
    fn relative_horizontal_vertical() {
        let cmds = parse_svg_path_data("M 10 20 h 5 v -3").unwrap();
        match cmds[1] {
            PathCommand::LineTo(p) => assert_point(p, 15.0, 20.0),
            _ => panic!("expected LineTo for h"),
        }
        match cmds[2] {
            PathCommand::LineTo(p) => assert_point(p, 15.0, 17.0),
            _ => panic!("expected LineTo for v"),
        }
    }

    #[test]
    fn cubic_bezier() {
        let cmds = parse_svg_path_data("M 0 0 C 10 20 30 40 50 60").unwrap();
        assert_eq!(cmds.len(), 2);
        match cmds[1] {
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                assert_point(control1, 10.0, 20.0);
                assert_point(control2, 30.0, 40.0);
                assert_point(to, 50.0, 60.0);
            }
            _ => panic!("expected CubicTo"),
        }
    }

    #[test]
    fn smooth_cubic() {
        let cmds = parse_svg_path_data("M 0 0 C 10 20 30 40 50 60 S 80 90 100 110").unwrap();
        assert_eq!(cmds.len(), 3);
        // S reflects previous control2 (30,40) across current point (50,60) → (70,80)
        match cmds[2] {
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                assert_point(control1, 70.0, 80.0);
                assert_point(control2, 80.0, 90.0);
                assert_point(to, 100.0, 110.0);
            }
            _ => panic!("expected CubicTo from S"),
        }
    }

    #[test]
    fn quadratic_bezier() {
        let cmds = parse_svg_path_data("M 0 0 Q 10 20 30 40").unwrap();
        assert_eq!(cmds.len(), 2);
        match cmds[1] {
            PathCommand::QuadTo { control, to } => {
                assert_point(control, 10.0, 20.0);
                assert_point(to, 30.0, 40.0);
            }
            _ => panic!("expected QuadTo"),
        }
    }

    #[test]
    fn smooth_quad() {
        let cmds = parse_svg_path_data("M 0 0 Q 10 20 30 30 T 60 50").unwrap();
        assert_eq!(cmds.len(), 3);
        // T reflects previous control (10,20) across current point (30,30) → (50,40)
        match cmds[2] {
            PathCommand::QuadTo { control, to } => {
                assert_point(control, 50.0, 40.0);
                assert_point(to, 60.0, 50.0);
            }
            _ => panic!("expected QuadTo from T"),
        }
    }

    #[test]
    fn close_path() {
        let cmds = parse_svg_path_data("M 0 0 L 10 10 Z").unwrap();
        assert_eq!(cmds.len(), 3);
        assert!(matches!(cmds[2], PathCommand::Close));
    }

    #[test]
    fn implicit_lineto_after_moveto() {
        // Per SVG spec, coordinates after M without a new command become L
        let cmds = parse_svg_path_data("M 0 0 10 20 30 40").unwrap();
        assert_eq!(cmds.len(), 3);
        assert!(matches!(cmds[0], PathCommand::MoveTo(_)));
        assert!(matches!(cmds[1], PathCommand::LineTo(_)));
        assert!(matches!(cmds[2], PathCommand::LineTo(_)));
    }

    #[test]
    fn arc_produces_cubics() {
        let cmds = parse_svg_path_data("M 0 0 A 25 25 0 0 1 50 0").unwrap();
        // Should produce MoveTo + one or more CubicTo
        assert!(cmds.len() >= 2);
        assert!(matches!(cmds[0], PathCommand::MoveTo(_)));
        for cmd in &cmds[1..] {
            assert!(
                matches!(cmd, PathCommand::CubicTo { .. }),
                "arc should produce CubicTo, got {cmd:?}"
            );
        }
        // Last point should be approximately (50, 0)
        if let PathCommand::CubicTo { to, .. } = cmds.last().unwrap() {
            assert_point(*to, 50.0, 0.0);
        }
    }

    #[test]
    fn degenerate_arc_zero_radius() {
        let cmds = parse_svg_path_data("M 0 0 A 0 0 0 0 1 50 0").unwrap();
        // Zero radius → lineto
        assert_eq!(cmds.len(), 2);
        assert!(matches!(cmds[1], PathCommand::LineTo(_)));
    }

    #[test]
    fn comma_separated() {
        let cmds = parse_svg_path_data("M10,20L30,40").unwrap();
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn negative_implicit_separator() {
        // Negative sign acts as separator
        let cmds = parse_svg_path_data("M10-20L30-40").unwrap();
        assert_eq!(cmds.len(), 2);
        match cmds[0] {
            PathCommand::MoveTo(p) => assert_point(p, 10.0, -20.0),
            _ => panic!("expected MoveTo"),
        }
    }

    #[test]
    fn scientific_notation() {
        let cmds = parse_svg_path_data("M 1e2 2.5e1").unwrap();
        match cmds[0] {
            PathCommand::MoveTo(p) => assert_point(p, 100.0, 25.0),
            _ => panic!("expected MoveTo"),
        }
    }

    #[test]
    fn real_world_material_icon() {
        // Material Design "check" icon path
        let d = "M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z";
        let cmds = parse_svg_path_data(d).unwrap();
        assert!(!cmds.is_empty());
        assert!(matches!(cmds[0], PathCommand::MoveTo(_)));
        assert!(matches!(cmds.last().unwrap(), PathCommand::Close));
    }

    #[test]
    fn empty_string() {
        let cmds = parse_svg_path_data("").unwrap();
        assert!(cmds.is_empty());
    }

    #[test]
    fn whitespace_only() {
        let cmds = parse_svg_path_data("   \t\n  ").unwrap();
        assert!(cmds.is_empty());
    }
}
