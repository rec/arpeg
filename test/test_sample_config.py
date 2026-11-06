from fractions import Fraction
from typing import TYPE_CHECKING

from ufor.arpeggiator import Grid, Strum

from arpeg.engine import EngineSettings

if TYPE_CHECKING:
    from reccy.pytest_plugin import SampleConfigRegression

pytest_plugins = ["reccy.pytest_plugin"]


class ExampleSettings(EngineSettings, frozen=True):
    rhythm: Grid = Grid(step="1/4 beat", groove=[Fraction(1)])
    strum: Strum | None = Strum(spacing="1/32 beat")


def test_sample_config(sample_config_regression: "SampleConfigRegression") -> None:
    sample_config_regression(ExampleSettings)
