"""Optional, host-clock connections to the standalone enveLFO scheduler."""

from fractions import Fraction
from typing import Self

from envelfo import patch
from pydantic import BaseModel, model_validator
from ufor import arpeggiator_ports, control, envelfo
from ufor.arpeggiator import ExpressionLane


class ExpressionObservation(BaseModel, frozen=True):
    values: dict[str, float]
    controls: list[arpeggiator_ports.ArpeggiatorControl]
    events: list[envelfo.EnvelfoOutputEvent]


class EnveLFOBridge(BaseModel):
    """Host-owned Patch state with one hit target and explicit expression lanes.

    Deliver each hit at its host timestamp before observing that timestamp.
    Supply returned controls to the player's realization phase before note-on.
    No transport reset, note-off routing, or observation cadence is implicit.
    """

    definition: patch.PreparedPatch
    state: patch.PatchState
    hit_target: str
    expression: dict[ExpressionLane, str]

    @model_validator(mode="after")
    def check_connections(self) -> Self:
        if (
            not isinstance(
                self.definition.envelfos.get(self.hit_target), envelfo.Envelope
            )
            or self.hit_target in self.definition.inputs
        ):
            raise ValueError("hit target must be a Patch-owned Envelope")
        if any(n not in self.definition.outputs for n in self.expression.values()):
            raise ValueError("expression mappings require named Patch outputs")
        return self

    def receive(
        self,
        at: Fraction,
        event: arpeggiator_ports.ArpeggiatorOutput,
        *,
        tempo_map: control.TempoMap | None = None,
    ) -> list[envelfo.EnvelfoOutputEvent]:
        """Translate one hit, using host seconds rather than the event's beat."""
        if event.port != arpeggiator_ports.ArpeggiatorOutputPort.hit:
            return []
        ordinal = self.state.children[self.hit_target].runtime.ordinal + 1
        state, events = patch.patch_event(
            self.definition,
            self.state,
            self.hit_target,
            envelfo.EnvelfoEvent(at=at, ordinal=ordinal, action="start"),
            tempo_map=tempo_map,
            inclusive=False,
        )
        self.state = state
        return events

    def observe(
        self,
        at: Fraction,
        *,
        next_delivery: Fraction,
        tempo_map: control.TempoMap | None = None,
    ) -> ExpressionObservation:
        """Observe once after same-time hits; leave lane ownership to arpeg."""
        result = patch.advance_patch(
            self.definition,
            self.state,
            at,
            next_delivery=next_delivery,
            tempo_map=tempo_map,
        )
        controls = [
            arpeggiator_ports.ArpeggiatorControl(
                port=arpeggiator_ports.ArpeggiatorInputPort(p.value),
                value=Fraction(str(result.values[n])),
            )
            for p, n in self.expression.items()
        ]
        self.state = result.state
        return ExpressionObservation(
            values=result.values, controls=controls, events=result.events
        )
