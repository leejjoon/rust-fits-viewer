from visualize_test_case import visualize_test_case
from generate_test_cases import TestCaseGenerator


generator1 = TestCaseGenerator(
    render_size=(800, 600),
    image_size=(800, 600),
    tile_size=256
)

cases_simple = [
    generator1.generate_test_case(
        name="simple01",
        zoom=1.0,
        pan_offset=(0.0, 0.0),
    ),
    generator1.generate_test_case(
        name="simple02",
        zoom=1.0,
        pan_offset=(700.0, 700.0),
    ),
    generator1.generate_test_case(
        name="simple03",
        zoom=1.0,
        pan_offset=(300.0, 0.0),
    ),
    generator1.generate_test_case(
        name="simple04",
        zoom=1.0,
        pan_offset=(100.0, 500.0),
    ),
]



generator1 = TestCaseGenerator(
    render_size=(800, 600),
    image_size=(2048, 2048),
    tile_size=256
)

cases_large = [
    generator1.generate_test_case(
        name="large01",
        zoom=1.0,
        pan_offset=(0.0, 0.0),
    ),
    generator1.generate_test_case(
        name="large02",
        zoom=1.0,
        pan_offset=(600.0, 0.0),
    ),
    generator1.generate_test_case(
        name="large03",
        zoom=1.0,
        pan_offset=(600.0, 300.0),
    ),
]


generator1 = TestCaseGenerator(
    render_size=(500, 500),
    image_size=(4096, 4096),
    tile_size=256
)

cases_lod = [
    generator1.generate_test_case(
        name="lod01",
        zoom=1.0,
        pan_offset=(0.0, 0.0),
    ),
    generator1.generate_test_case(
        name="lod01",
        zoom=0.9,
        pan_offset=(0.0, 0.0),
    ),
    generator1.generate_test_case(
        name="lod02",
        zoom=0.7,
        pan_offset=(0.0, 0.0),
    ),
    generator1.generate_test_case(
        name="lod03",
        zoom=0.3,
        pan_offset=(0.0, 0.0),
    ),
    generator1.generate_test_case(
        name="lod04",
        zoom=0.1,
        pan_offset=(0.0, 0.0),
    ),
    generator1.generate_test_case(
        name="lod05",
        zoom=0.05,
        pan_offset=(0.0, 0.0),
    ),
    generator1.generate_test_case(
        name="lod06",
        zoom=0.01,
        pan_offset=(0.0, 0.0),
    ),
]


generator1 = TestCaseGenerator(
    # render_size=(517, 298),
    render_size=(600, 400),
    image_size=(1024, 1024),
    tile_size=256
)

cases_fail = [
    generator1.generate_test_case(
        name="fail01",
        zoom=0.586,
        pan_offset=(0.0, 0.0),
    ),
]

cases_miss = [
    generator1.generate_test_case(
        name="miss01",
        zoom=1.1443,
        pan_offset=(343.8, 468.),
    ),
]


cases = cases_miss

if True:
    import matplotlib.pyplot as plt
    fig = plt.figure(1)
    fig.clf()
    visualize_test_case(cases[-1], fig=fig)
    plt.show()

if False:
    import os
    import json
    from pathlib import Path

    testdir = Path("client/fits_view_client/tests/test_cases")
    testdir.mkdir(exist_ok=True)

    # for case in cases_miss:
    for case in cases_simple + cases_large + cases_lod:
        filename = case["name"] + ".json"
        filepath = testdir / filename
        with open(filepath, 'w') as f:
            json.dump(case, f, indent=2)
        print(f"Generated {filepath}")
