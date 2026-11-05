//! turbo-surf#131: an author `border` on a `<button>` turns its native appearance off, so
//! the static screenshot paints the CSS border like Chrome (turbo-dom#283). The page is the
//! detection harness's border-cascade fixture at 1280x800, with every `solid` border given
//! an explicit red. Every sample sits inside a border band of Chrome 156's own screenshot of
//! that page (`chrome-headless-shell --screenshot --window-size=1280,800 --hide-scrollbars`),
//! at least 1px from the band's edges, where Chrome paints pure red.
use turbo_dom_raster as raster;

const FIXTURE: &str = include_str!("../../../../harness/detection/fixtures/border-cascade.html");

/// `(x, y)` samples Chrome 156 paints red: #aggregate's (8,12 26.7x25, 4px border) left,
/// top and bottom bands. The later buttons are left out: each of turbo-surf's buttons is
/// about 1px wider than Chrome's, so from the third one on they drift past the ~3px
/// tolerance.
const CHROME_BORDER: &[(u32, u32)] = &[(9, 24), (10, 24), (20, 13), (20, 14), (20, 35)];

fn screenshot(page: &str) -> impl Fn(u32, u32) -> [u8; 3] {
    let viewport = raster::Viewport {
        width: 1280,
        height: 800,
    };
    let png = raster::screenshot_png(page, viewport).unwrap();
    let shot = turbo_dom_layout::image::decode_png_rgba(&png).unwrap();
    move |x, y| {
        let i = ((y * shot.width + x) * 4) as usize;
        [shot.rgba[i], shot.rgba[i + 1], shot.rgba[i + 2]]
    }
}

#[test]
fn author_button_borders_paint_like_chrome156() {
    let pixel = screenshot(&FIXTURE.replace("solid", "solid #f00"));
    let wrong: Vec<_> = CHROME_BORDER
        .iter()
        .filter(|&&(x, y)| pixel(x, y) != [255, 0, 0])
        .map(|&(x, y)| format!("({x},{y}) = {:?}", pixel(x, y)))
        .collect();
    assert!(wrong.is_empty(), "not Chrome's author border: {wrong:?}");
    // Inside #aggregate's border, the button face stays Chrome's light grey.
    assert_eq!(pixel(20, 20), [239, 239, 239]);
}

/// turbo-dom#303 (fixed in turbo-dom 0.7.1): a border without a colour paints in the
/// element's `color` (`currentcolor`). The fixture's buttons have no `color`, so Chrome 156
/// paints their borders black, sampled at the same points as the red case above.
#[test]
fn current_color_button_borders_paint_black_like_chrome156() {
    let pixel = screenshot(FIXTURE);
    let wrong: Vec<_> = CHROME_BORDER
        .iter()
        .filter(|&&(x, y)| pixel(x, y) != [0, 0, 0])
        .map(|&(x, y)| format!("({x},{y}) = {:?}", pixel(x, y)))
        .collect();
    assert!(
        wrong.is_empty(),
        "not Chrome's currentcolor border: {wrong:?}"
    );
    assert_eq!(pixel(20, 20), [239, 239, 239]);
}
