global _start
section .text
_start:
    call helper
    jmp finish
skipped:                    ;~
    nop
finish:
    mov rax, [value]
    mov rax, 60
    syscall
helper:
.local:                     ;~
    ret
main:
    ret

section .data
value dq 1
spare dq 2                  ;~
SIZE equ 8                  ;~
