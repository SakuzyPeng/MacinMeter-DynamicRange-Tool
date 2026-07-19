; Raw floating-point control access for Windows x64.
;
; The worker records and restores the x87 control word and MXCSR exactly.
; MASM is used because MSVC-style inline assembly is unavailable on x64.

PUBLIC mm108_store_x87_control_word
PUBLIC mm108_load_x87_control_word
PUBLIC mm108_store_mxcsr
PUBLIC mm108_load_mxcsr

.code

mm108_store_x87_control_word PROC
    fnstcw WORD PTR [rcx]
    ret
mm108_store_x87_control_word ENDP

mm108_load_x87_control_word PROC
    sub rsp, 16
    mov WORD PTR [rsp], cx
    fnclex
    fldcw WORD PTR [rsp]
    add rsp, 16
    ret
mm108_load_x87_control_word ENDP

mm108_store_mxcsr PROC
    stmxcsr DWORD PTR [rcx]
    ret
mm108_store_mxcsr ENDP

mm108_load_mxcsr PROC
    sub rsp, 16
    mov DWORD PTR [rsp], ecx
    ldmxcsr DWORD PTR [rsp]
    add rsp, 16
    ret
mm108_load_mxcsr ENDP

END
