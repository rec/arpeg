"""Reproducible, independently named random choices without shared RNG state."""


def draw_below(
    seed: int, name: str, lane: str, revision: int, decision: int, bound: int
) -> int:
    """Draw uniformly below a positive bound using the documented v1 mixer."""
    bits = (bound - 1).bit_length()
    retry = 0
    while True:
        value = 0
        for chunk in range((bits + 63) // 64):
            text = (
                f"arpeg-v1:{seed}:{len(name.encode())}:{name}:{lane}:"
                f"{revision}:{decision}:{retry}:{chunk}"
            )
            word = 0xCBF29CE484222325
            for byte in text.encode():
                word = ((word ^ byte) * 0x100000001B3) & _MASK
            word = ((word ^ (word >> 30)) * 0xBF58476D1CE4E5B9) & _MASK
            word = ((word ^ (word >> 27)) * 0x94D049BB133111EB) & _MASK
            value |= (word ^ (word >> 31)) << (chunk * 64)
        value &= (1 << bits) - 1
        if value < bound:
            return value
        retry += 1


_MASK = (1 << 64) - 1
