# Motion files

A motion file writes down a camera motion, so that the same movie can be made again and again, for a demo or a figure.

## How to use

1. Open a structure (for example `../structures/1ubq.pdb`).
2. File → "Export Animation...".
3. Press "Choose file..." and pick a file from this folder. The whole file is checked at once; a mistake is shown in the dialog with the step number. When the file is valid, "Motion file" is selected, and the frame count and the frame rate come from the file.
4. Choose the width and the rest, then start (desktop: choose a folder; browser: a zip is downloaded).

## Format

YAML. `schema: yui-motion/1`, `fps` (1 to 120) and `steps`, which run from top to bottom. The whole motion may be at most 600 s.

| Step | Keys | Meaning |
|---|---|---|
| `turn` | `axis`, `angle`, `seconds` | Turn the molecule about an axis through the target. `axis` is `x`, `y` or `z` (the screen's right, up and toward-you axes) or any vector `[x, y, z]`. `angle` is in degrees; counter-clockwise looking from the tip of the axis is positive. |
| `zoom` | `factor`, `seconds` | Multiply the distance from the camera to the target by `factor`: under 1 comes closer, over 1 goes away. |
| `wait` | seconds | Stay still. |

Each step moves at a constant speed, and the next step starts where the last one ended.

Not yet: stepping through the models or the frames of a trajectory, and changing representations during the movie.

## Files

| File | What it does |
|---|---|
| `turn-around.yaml` | One turn around the vertical axis (6 s), a pause, a zoom in, a tilt forward by 90°, and a last pause (12 s in all, 360 frames at 30 fps). |
