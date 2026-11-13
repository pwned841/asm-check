global _start
section .text
_start:
.loop:
    jmp .loop
done:
    mov rax, 60
    syscall
other:
.loop:
    jmp .loop
done:                       ;~
    ret

section .data
msg db 1
msg db 2                    ;~
