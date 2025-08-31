# FITS Viewer - Keyboard and Mouse Bindings

## Mouse Controls

### Mouse Drag (Pan)
- **Left click + drag**: Pan the image around the viewport
- The Y-axis is corrected so dragging up moves the image up (not inverted)
- Uses 1:1 pixel-to-coordinate mapping within the render area

### Mouse Scroll (Zoom/Rotate)
- **Scroll wheel**: Zoom in/out (zoom range: 0.1x to 10.0x)
- **Alt + Scroll wheel**: Rotate the image clockwise/counter-clockwise
  - Scroll up = counter-clockwise rotation
  - Scroll down = clockwise rotation
- Both controls only work when the mouse is hovering over the image area

## Keyboard Shortcuts

### R Key: Reset viewport
- Resets zoom to 1.0x, pan offset to (0,0), and rotation to 0°

### F Key: Fit to window
- Automatically scales the image to fit within the window while maintaining aspect ratio
- Centers the image and resets rotation

### Q Key: Quit application
- Closes the application window

## Implementation Details

- **Zoom sensitivity**: 0.001 per scroll unit
- **Rotation sensitivity**: 0.005 radians per scroll unit when Alt is held
- **Pan sensitivity**: Dynamically calculated based on render area size for precise control
- All transformations are applied in real-time with immediate visual feedback

## Technical Notes

**Why Alt+Scroll instead of Ctrl+Scroll?**
The system/desktop environment consumes Ctrl+scroll events before they reach the application, making Ctrl+scroll unavailable for rotation control. Alt+scroll provides the same functionality without system interference.

## Usage Notes

The controls provide a standard image viewer experience with pan, zoom, and rotate functionality, plus convenient reset and fit-to-window shortcuts. All mouse interactions are constrained to the image rendering area for precise control.
