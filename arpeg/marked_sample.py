"""Lower frame markers to portable sample notes without loading audio."""

from typing import Literal, Self

from pydantic import BaseModel, Field, model_validator
from ufor.arpeggiator_capture import CapturedPhrase, SourceNote
from ufor.base import Identifier
from ufor.samples.playback import Slice
from ufor.time import Timebase


class SampleMarker(BaseModel, frozen=True):
    note_id: Identifier
    selection_key: int = Field(strict=True)
    at_frame: int = Field(strict=True, ge=0)
    gate_end_frame: int | None = Field(default=None, strict=True, ge=0)


class MarkedSample(BaseModel, frozen=True):
    """An exhaustive, source-ordered bank of marked regions in one asset."""

    capture_id: Identifier
    asset: Identifier
    sample_rate: int = Field(strict=True, gt=0)
    frames: int = Field(strict=True, gt=0)
    channels: list[int] = Field(min_length=1)
    markers: list[SampleMarker] = Field(min_length=1)

    @model_validator(mode="after")
    def valid_regions(self) -> Self:
        if self.markers[0].at_frame != 0:
            raise ValueError(
                "an exhaustive sample bank requires a marker at frame zero"
            )
        if self.markers[-1].at_frame >= self.frames or any(
            b.at_frame <= a.at_frame
            for a, b in zip(self.markers, self.markers[1:], strict=False)
        ):
            raise ValueError("sample markers must increase within the asset")
        if len({m.note_id for m in self.markers}) != len(self.markers):
            raise ValueError("sample marker note IDs must be unique")
        if any(
            m.gate_end_frame is not None and not m.at_frame < m.gate_end_frame <= end
            for m, end in zip(
                self.markers,
                [*(n.at_frame for n in self.markers[1:]), self.frames],
                strict=True,
            )
        ):
            raise ValueError("sample gate must end within its region")
        if len(set(self.channels)) != len(self.channels) or any(
            c < 0 for c in self.channels
        ):
            raise ValueError("sample channels must be unique nonnegative indices")
        return self

    def phrase(self) -> CapturedPhrase:
        """Keep source frames and selection keys separate from acoustic pitch."""
        notes = [
            SourceNote(
                capture_id=self.capture_id,
                note_id=marker.note_id,
                onset_tick=marker.at_frame,
                gate_end_tick=marker.gate_end_frame or end,
                cell_end_tick=end,
                selection_key=marker.selection_key,
                region=Slice(
                    name=marker.note_id,
                    asset=self.asset,
                    start_frame=marker.at_frame,
                    end_frame=end,
                ),
            )
            for marker, end in zip(
                self.markers,
                [*(m.at_frame for m in self.markers[1:]), self.frames],
                strict=True,
            )
        ]
        return CapturedPhrase(
            capture_id=self.capture_id,
            timebase=Timebase.model_validate(
                {"name": "source-frames", "rate": {"numerator": self.sample_rate}}
            ),
            end_tick=self.frames,
            notes=notes,
        )

    def select(
        self,
        order: Literal["ascending", "descending", "played", "reverse_played"],
        cycles: int = 1,
    ) -> list[SourceNote]:
        """Return identified notes in selection order, including repeated cycles."""
        if cycles < 1:
            raise ValueError("selection requires at least one cycle")
        notes = self.phrase().notes
        if order == "reverse_played":
            notes.reverse()
        elif order != "played":
            notes.sort(
                key=lambda n: (n.selection_key, n.onset_tick),
                reverse=order == "descending",
            )
        return notes * cycles
