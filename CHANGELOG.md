# Changelog

All notable changes to VeloMov. *Versão em português: [CHANGELOG.pt-BR.md](./CHANGELOG.pt-BR.md).*

## 0.9.0 — 2026-10-23

### Fonts

* **Font previews**: the font list draws every font name in that font (with the same text engine
  as the video, so what you see is what you get). Above the list, a large preview shows the clip's
  own text in the font under the mouse, with the clip's bold/italic.
* **Favourite fonts**: click the ☆ next to a font to keep it in a **Favourites** section at the top
  of the list. The ☆ next to the search box shows only the favourites.
* **Recently used fonts**: the last 8 fonts you picked appear in **Recently used**, right below the
  favourites.
* **Settings → General → Text fonts** shows how many favourites and recent fonts there are, lets
  you remove a favourite and clear either list.
* The font list only draws the rows on screen, so it opens instantly even with thousands of fonts.

### A new home screen

* **Welcome banner**: a slowly moving "aurora" in the gradient colours, a greeting that follows the
  time of day, how many projects you have and how much video they add up to, and the main actions
  (**Create project**, **Import media**, **Demo project**).
* A little animated timeline (ruler, clips and playhead) next to a **Continue editing** card that
  opens the project you edited last.
* **Tip of the day** at the bottom of the banner (click it for the next tip).
* **Your projects** can be shown as a **grid** or a **list**, ordered by **last edited**, **name**
  or **duration**. Cards lift on hover and show a play button; list rows have a **…** menu.
* New empty state: a drop zone that also creates the first project when clicked.
* Navigation (**Open project**, **Settings**, **About**) moved to a top bar — it can go back to a
  side bar on the left or on the right (see below).
* Dates read "yesterday" instead of "1 days ago".

### Settings in tabs

* The settings window now has tabs down its left side: **Appearance**, **Layout**, **General**,
  **Performance**, **Captions** and **Storage**. Each tab has a short description and, where it
  makes sense, its own **Restore defaults** button.

### Customise the interface

**Appearance**

* **Gradient** (Export button, home banner, other highlights):
  * *Automatic* — from the accent colour to a neighbouring hue; the **second colour** slider moves
    it around the colour wheel (−180° to 180°).
  * *Two colours* — any two colours, with a **Swap** button.
  * *Off* — no gradient, only the accent colour.
  * **Direction**: horizontal, diagonal or vertical. A live preview bar and sample button show the
    result.
* **Rounded corners** of panels, windows and buttons (0 = square, up to 20).
* **Space between panels** (0–12).
* **Panel borders**: a thin outline around each panel.
* **High-contrast text**: brighter secondary text.

**Layout**

* **Interface size**: 80 % to 150 % (handy on high-resolution screens).
* **Editor**: media library on the **left** or **right** (the inspector goes to the other side),
  timeline at the **bottom** or **top**, and **track height** (compact, normal, large). A small
  drawing shows the chosen layout.
* **Home screen**: navigation in a **top bar**, **left side** or **right side**; **welcome banner**
  on or off; projects as **grid** or **list**; **card size** (small, normal, large).
* **Notifications** at the top or bottom of the window.
* **Animations** on or off (menus, card hover, the moving home background).

Every option applies at once, without restarting, and is saved with the other settings.

### Compatibility

* Settings from 0.8 open with all the new options at their defaults. The project format did not
  change.

## 0.8.1 — 2026-10-22 (pre-release)

* Projects are saved as `.vmov` files; `.vcut` projects from 0.8.0 and older still open and are
  renamed when possible.
* Projects saved elsewhere with **Save as** show up on the home screen; covers of those projects are
  kept in VeloMov's data folder instead of next to the project.
* Keyframe diamonds on the timeline are drawn on top of trim handles and transitions.

## 0.8.0 — 2026-10-22

* VeloCut is now **VeloMov**.
* **Animated auto captions** with whisper.cpp, on your computer (GPU through Vulkan when
  available), in 12 styles, with model choice in Settings.
* **Windows** version: installer and portable zip.
* Fix: the transform gizmo of the player no longer starts a drag from outside the player.
