; Highlights for the Patches DSL.
;
; Tap targets (ADR 0054, ticket 0695):
;   ~taptype+component+...(name)
;
; Tree-sitter highlight captures map to TextMate-style scopes via the
; consuming editor's theme. The scope names below follow the standard
; tree-sitter highlight vocabulary so themes pick them up out of the box.

; The leading `~` punctuator on a tap target.
(tap_target "~" @punctuation.special)

; Tap component names (`meter`, `osc`, `spectrum`, `gate_led`, `trigger_led`,
; `stereo_meter`). The set is open at parse-time and validated post-parse.
(tap_component (ident) @function.special)

; The tap's identifier (the bare ident inside the parens, after the
; components). Identified positionally as the only direct ident child of
; tap_target.
(tap_target (ident) @variable)

; The `stereo` prefix on stereo-paired module declarations (ADR 0070).
(stereo_kw) @keyword
