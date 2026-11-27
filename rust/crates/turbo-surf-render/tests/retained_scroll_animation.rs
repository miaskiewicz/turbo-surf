use serde_json::Value;
use turbo_surf_render::PageSession;

const HTML: &str = include_str!("fixtures/retained-scroll-animation.html");

#[tokio::test(flavor = "current_thread")]
async fn clipped_sibling_animation_preserves_rectangles_with_retained_scroll() {
    for (setup, y, window_y, inner_y) in [
        ("", 8.0, 0, 0),
        ("window.scrollTo(0,20);", -12.0, 20, 0),
        (
            "document.querySelector('#scroll').scrollTop=10;",
            8.0,
            0,
            10,
        ),
    ] {
        let mut page = PageSession::open(HTML, "https://clip.test/", "", "", 5000)
            .await
            .unwrap();
        let result = page
            .read_engine(&format!(
                "{setup}const windowY=scrollY,innerY=document.querySelector('#scroll').scrollTop;document.querySelector('#animated').className='active';const r=document.querySelector('#target').getBoundingClientRect();return {{x:r.x,y:r.y,width:r.width,height:r.height,windowY,innerY}};"
            ))
            .unwrap_or_else(|error| panic!("setup={setup}: {error}"));
        let result: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["x"], 8.0);
        assert_eq!(result["y"], y);
        assert_eq!(result["width"], 200.0);
        assert_eq!(result["height"], 40.0);
        assert_eq!(result["windowY"], window_y);
        assert_eq!(result["innerY"], inner_y);
        page.close();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn retained_scroll_keeps_unproven_animation_dependencies_unsupported() {
    for mutation in [
        "document.querySelector('#clip').style.overflow='visible';",
        "document.querySelector('#clip').style.display='contents';",
        "document.querySelector('#clip').style.cssText='display:inline;position:relative;overflow:hidden';",
        "document.querySelector('#clip').style.position='static';",
        "document.querySelector('#clip').appendChild(document.querySelector('#target'));",
        "const child=document.createElement('div');child.style.position='fixed';document.querySelector('#animated').appendChild(child);",
        "const popup=document.createElement('div');popup.setAttribute('popover','manual');popup.style.position='absolute';document.querySelector('#clip').appendChild(popup);popup.showPopover();window.scrollTo(0,20);",
        "const dialog=document.createElement('dialog');dialog.style.position='absolute';document.querySelector('#clip').appendChild(dialog);dialog.showModal();window.scrollTo(0,20);",
    ] {
        let mut page = PageSession::open(HTML, "https://clip.test/", "", "", 5000)
            .await
            .unwrap();
        let error = page.read_engine(&format!(
            "window.scrollTo(0,20);{mutation}document.querySelector('#animated').className='active';return document.querySelector('#target').getBoundingClientRect().width;"
        )).unwrap_err();
        assert!(error.contains("animation resource is unsupported: public-ripple"), "{mutation}: {error}");
        page.close();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn animated_subject_is_not_admitted_by_its_own_clipping_ancestor() {
    let mut page = PageSession::open(HTML, "https://clip.test/", "", "", 5000)
        .await
        .unwrap();
    let error = page.read_engine("window.scrollTo(0,20);const animated=document.querySelector('#animated');animated.className='active';return animated.getBoundingClientRect().width;")
        .unwrap_err();
    assert!(error.contains("animation resource is unsupported: public-ripple"));
    page.close();
}
