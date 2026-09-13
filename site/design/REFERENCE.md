# Site reference set, 2026-09-13

Measured live with Chrome DevTools at 1440x900 (DPR 1) and 390x844 (DPR 2, mobile) on 2026-09-13. Values are computed styles read from the page, not from the stylesheet. Screenshots at both widths sit in `.status/site-1-0/refs/` in the main checkout (gitignored).

## Per site

### granola.ai
- H1 "The AI notepad for back-to-back meetings" (6 words), 86px, weight 400 in a serif display (quadrant), tracking -0.02em, line-height 0.93, left aligned in a two-column hero. 390: same face, wraps to four lines.
- Sub: "Notes, actions and memory. Without a meeting bot." at 20px, near-black.
- CTA pair: one filled pill "Download for free" (olive #5B6F00 on off-white, 56px tall); no second button. Detail line under it: "Available for macOS, Windows, iOS, Android" at 13px with a check glyph.
- Media: the app window is a real window at 2x, offset right and cropped by the viewport edge, with textured collage art behind it. Distance from CTA to media is zero: they share the fold side by side.
- Section rhythm: 13 `<section>` elements, headings at 48 to 160px, all weight 400, alternating text-left and text-right. Page height 14306px.
- Container: 1280px (47 elements), a 1200 inner. Type scale: 13/14/16/17/18/20/22/24/32/36/48/60/86.
- Colours: 26 distinct, three do the work (near-black #292929, off-white #F7F7F2, olive accent). Everything else is the collage art.
- Footer: 29 links across Features / Product / Company / Resources, no columns markup, one tagline-free block.

### linear.app
- H1 "The product development system for teams and agents" (8 words), 64px, weight 510 in Inter Variable, tracking -0.022em, line-height 1.0, left aligned. 390: 38px, weight 510, tracking -0.022em, line-height 1.1.
- Sub: 11 words at 15px in a grey (#8A8F98) on near-black.
- CTA pair: on the home page the hero carries no button at all; the nav owns "Sign up" (filled pill) and "Log in" (text). A "New Loops →" link sits right of the sub.
- Media: the full app window at 1440px wide, hairline border, no radius at the top, no shadow, cropped by the fold. Distance from sub to media: 72px.
- Section rhythm: 8 sections, H2 at 48px/510, one closing H2 at 72px. Page height 9954px.
- Container: 1333px hero, 784px text measure, 1310px media. Type scale: 10/11/12/13/14/15/16/18/20/24/32/48/64/72 (14 steps).
- Colours: 47 distinct, one accent (#5E6AD2) used on a handful of controls. Nav: fixed, transparent, 1px bottom hairline at 8% white, 73px tall.
- Footer: 42 links in 6 columns.

### cap.so
- H1 "Record. Edit. Share." (3 words), 74px, weight 400 in Instrument Sans, tracking -0.03em, line-height 0.98, centered. 390: 36px, tracking -0.03em.
- Sub: 35 words (too long) at 19px, 78% black, serif (Source Serif 4), 660px wide.
- CTA pair: "Download free for macOS" (outlined, 48px tall, 12px radius) and "See how Cap works" (white, 10px radius). Detail lines under: "Switching from Loom? Import your library →" and "Also available on" with three OS icons.
- Media: a full macOS desktop frame at 1084px wide, radius 14px inside a 22px padded bezel, 22px below the last detail line.
- Section rhythm: 17 sections, H2 at 48 to 56px, weight 400, each with its own media. Page height 19267px.
- Container: 1200px (24 elements), text 760px. Type scale: 32 distinct sizes, from 9.6 to 78px, half of them half-pixel steps.
- Colours: 108 distinct. Canvas #F2F2F2 with white cards.
- Footer: 53 links in 5 columns plus a tagline paragraph.

### cron.com
- The domain now serves a one-screen placeholder (Cron became Notion Calendar): H1 "It's about time." at 140px Helvetica Neue 700, one sub at 22px, one image, 4 footer links, 5 colours in total. No sections, no CTA. Recorded as a stub; it contributes nothing to the lock.

### tuple.app
- Visible headline is a second `<h1>` (the first is the logo): "The best remote pair programming app on macOS and Windows" (10 words), 60px, weight 600 in Inter Variable, tracking normal, centered. 390: 30px/600, line-height 1.2.
- Sub: 26 words at 18px grey (#71717A), 763px wide, centered.
- CTA: one filled button "Start your free trial" (#6A5ED9, 56px tall, 8px radius) and a 14px detail line "Free for 14 days. Cancel anytime." under it.
- Media: the app window at 1144px, radius 16px, one large shadow at 25% black, 120px under the detail line, cropped by the fold. On 390 it is 358px wide with a 12px radius.
- Section rhythm: no `<section>` tags; 8 H2 at 48px/600 with alternating white (#FFF), grey (#FAFAFA) and one dark (#09090B) band, band padding 128px.
- Container: 1152px (44 elements), text 768px. Type scale: 11/12/13/14/16/18/20/24/48/60 (10 steps).
- Colours: 44 distinct, one accent.
- Footer: 22 links in three groups plus a copyright line.

### raycast.com
- H1 "Your shortcut to everything." (4 words), 64px, weight 600 in Inter, tracking normal, line-height 1.1, centered over a red light-streak artwork. 390: wraps to two lines.
- Sub: 15 words at 18px white, 786px wide.
- CTA: one filled "Download for Mac" (36px tall, 8px radius), then two 12px mono detail lines: "macOS Tahoe and Apple Silicon required" and "Install via Homebrew | Download V1".
- Media: the artwork is the hero; the first product image is 1300px wide far below the fold.
- Section rhythm: no `<section>` tags; 8 H2 at 20px/500 (eyebrow-sized) with the real headline in a following element. Page height 15992px.
- Container: 1204px, text 733 to 750px. Type scale: 10/12/13/14/16/18/20/22/24/32/56/64.
- Colours: 39 distinct on a #07080A canvas. Nav is a floating rounded bar.
- Footer: none found as `<footer>`.

### craft.do
- H1 "Your space for notes, tasks, and big ideas" (8 words), 66px, weight 400 in a serif (Untitled Serif), tracking -0.03em, line-height 1.0, centered over an illustrated sky collage.
- No sub. One pill CTA "Try Craft Free" (48px tall) 105px above the app window, which is cut by the fold.
- Section rhythm: 7 sections, H2 at 54px/400. Page height 11056px.
- Container: 1080px (32 elements), a 1385px outer frame with a 20px page inset. Type scale: 18 steps.
- Colours: 42 distinct, pastel card fills inside the product shot only. Canvas #FCF9F7.
- Footer: 38 links in 9 columns, a heading per column.

### bear.app
- H1 "Markdown notes you'll love" (4 words), 51px, weight 400 in a custom sans, no tracking, centered. 390: 42px.
- Sub: 17 words at 22px grey (#888), 582px wide, with a Mac / iPhone / iPad platform switch under it.
- CTA: one outlined Mac App Store badge (no text button). No detail line.
- Media: the app window at 1258px, no radius or shadow of its own, 217px below the badge, cropped by the fold.
- Section rhythm: 8 sections, H2 at 41.6px/400 (the feature four) and 30.4px (the smaller three). Page height 10499px.
- Container: 896 to 960px text, 1200 to 1314px media. Type scale: 12.8/14.4/16/17.6/19.2/20/22.4/24/28.8/30.4/40/41.6/51.2 (rem multiples of 16).
- Colours: 11 distinct, the tightest in the set: #444 text, #888 secondary, white canvas, red brand, and the accent set used only inside the app shots.
- Footer: 19 links in 4 columns plus "Shiny Frog © 2025".

## What is common

- The H1 is 3 to 10 words, 51 to 86px at 1440, and names the category or the outcome. Six of seven live sites set it at weight 400 to 600; nobody bolds it. Five of seven track it between -0.02em and -0.03em.
- One filled CTA. Cap is the only site with a true button pair; Linear, Craft and Bear have zero or one button. The second action is a text link or a nav item.
- The detail line under the CTA is where the operational facts go (Raycast: OS requirement and Homebrew; Tuple: trial terms; Granola: platforms; Cap: other OSes).
- The product window is the first thing after the CTA and is cut by the fold on every site that has one: Linear at 72px under the sub, Tuple at 120px under the detail line, Bear at 217px under the badge, Cap at 22px. Every window is a real window at 2x.
- Sections run 7 to 17, H2 at 41 to 56px, weight 400 to 600, one media per section.
- Containers sit at 1080 to 1280px, the text measure at 730 to 900px.
- The tightest palettes (Bear 11, Granola 26 working colours) read as the calmest. Cap's 108 colours read as busy.
- Footers are 19 to 53 links; none carry a tagline that says anything (Cap's is marketing filler).

## The lock for Writ

Primary: **Tuple** for the page grammar (centered H1 at weight 600 in Inter, one sub in grey, one filled button with a detail line, the real window under it with one shadow, plain light bands between sections) with **Linear** for type discipline (Inter Variable at 510 to 600, tracking -0.022em, a 14-step scale, hairline nav) and **Bear** for palette count (11 colours) and for being a notes app that shows its window plainly.

Preserve:
- Centered hero, H1 at 44/60/72 in Inter 600, tracking -0.025em, line-height 1.05. Tuple's 60px H1 at 1440 is the direct model; Linear's 64 and Raycast's 64 confirm the range.
- One grey for secondary text (Tuple #71717A, Linear #8A8F98, Bear #888): Writ uses its own `fg.muted`.
- One filled button on the accent, a text button beside it (Cap's pair, Tuple's single filled button with a text detail line).
- The detail line in 13px under the buttons (Raycast, Tuple, Granola).
- The window as a real 2x capture, hairline border, one shadow, 64px under the buttons (between Linear's 72 and Cap's 22, the shadow from Tuple).
- Sections at 28 to 32px H2, one capture each, alternating sides (Granola's alternation, Tuple's band rhythm without the dark band).
- Footer as one row of links (Tuple's grouping without headings).

Borrow only:
- Linear's hairline under a solid nav.
- Bear's colour count as the ceiling.

Refuse:
- Granola's serif display and collage art, Craft's illustrated sky, Raycast's artwork hero, Cap's bezel and 108 colours, Tuple's dark band and mascots, Linear's dark canvas and its buttonless hero, Bear's platform switch and App Store badge.
- Cap's 35-word sub and 32-step type scale.
- Eyebrow H2s (Raycast's 20px section labels), badges ("New", "alpha is available"), cookie banners, marquees, testimonials, the floating pill nav.
- The page-as-buffer grammar of the page being replaced: source gutter, `##` tokens, spine, status-bar nav, typed hero, dark stage bands.

Token commitments: canvas `bg.canvas` #FFFFFF, text `fg.default` #1C1A17, secondary `fg.muted` #5D5850, hairline `border.default` #E4E1DB, accent Pine #1F6F5C on the primary button and links only, one radius 10px, one shadow `0 40px 80px -24px rgba(0,0,0,.25)` on app windows only, container 1120px, measure 60ch (68ch on prose pages), an 8px spacing scale, Inter 400/500/600 self-hosted, mono only inside `<code>`.
