# FITS Viewer Test Documentation

This document describes the testing strategies and implementation details for various components of the FITS Viewer.

## LOD Selection Test

### Intention

The primary goal of this test is to verify the correctness of the LOD (Level of Detail) selection logic, which is crucial for performance and visual quality. It ensures that the application requests the most appropriate tile resolution from the server based on the user's current zoom level. This prevents loading unnecessarily high-resolution data when zoomed out and avoids displaying blurry upscaled images when zoomed in.

### Implementation Details

The test suite is designed to be data-driven, using an external JSON file (`lod_test_cases.json`) to define a comprehensive set of test scenarios. This approach allows for easy extension and modification of test cases without changing the test code itself.

The test implementation covers two main aspects of the LOD selection logic:

1.  **Basic LOD Calculation:** This part of the test verifies the core formula `lod_to_request = round(-log2(s))`, where `s` is the zoom level. It includes test cases for various zoom levels, including 1:1, zoomed-in, zoomed-out, and edge cases that require rounding up or down. It also tests that the LOD level is clamped at 0 for zoom levels greater than 1.0.

2.  **Hysteresis:** This part of the test validates the mechanism for preventing rapid flickering of LOD levels when the zoom level is near a threshold. It uses the formula `abs(z_ideal - current_lod) > 0.5 + threshold` to determine if a change in LOD is warranted. The test cases cover scenarios where the LOD should change and where it should remain stable, based on different thresholds and current LODs.

The test code itself is located in `client/fits_view_client/tests/test_lod_selection.rs`. It reads the `lod_test_cases.json` file, deserializes the test cases into Rust structs, and then iterates through them, asserting that the calculated LOD matches the expected LOD for each case.
