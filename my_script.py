from visualize_test_case import visualize_test_case
from generate_test_cases import TestCaseGenerator


generator1 = TestCaseGenerator(
    render_size=(800, 600),
    image_size=(800, 600),
    tile_size=256
)

case1 = generator1.generate_test_case(
    name="Simple case: No zoom, no pan",
    zoom=1.0,
    pan_offset=(700.0, 700.0),
    lod=0
)

import matplotlib.pyplot as plt
fig = plt.figure(1)
fig.clf()
visualize_test_case(case1, fig=fig)
plt.show()
