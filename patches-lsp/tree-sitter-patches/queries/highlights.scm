; Highlights for the Patches DSL.
;
; Tap targets (ADR 0054, ticket 0695):
;   ~taptype(name, qualifier.key: value, ...)
;
; Tree-sitter highlight captures map to TextMate-style scopes via the
; consuming editor's theme. The scope names below follow the standard
; tree-sitter highlight vocabulary so themes pick them up out of the box.

; The leading `~` punctuator on a tap target.
(tap_target "~" @punctuation.special)

; Tap component names (`meter`, `osc`, `spectrum`, `gate_led`, `trigger_led`).
(tap_type) @function.special

; The tap's identifier (first arg inside the parens).
(tap_name (ident) @variable)

; Qualifier on a tap parameter (`meter` in `meter.window: 25`).
(tap_qualifier (ident) @property)

; Parameter key on a tap parameter (`window` in `window: 25` or
; `meter.window: 25`). The trailing ident in `tap_param_key` is the key;
; an optional preceding `tap_qualifier` is matched separately above.
(tap_param_key (ident) @variable.parameter)
