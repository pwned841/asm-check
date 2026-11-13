section .text
f:
    call message                ;~
    jmp buffer                  ;~
    je message                  ;~
    call f
    jmp .local
.local:
    ret

section .data
message db "x"

section .bss
buffer resb 8
