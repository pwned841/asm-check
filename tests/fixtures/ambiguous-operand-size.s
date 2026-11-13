section .text
sizes:
    mov [rdi], 0            ;~
    mov byte [rdi], 0
    mov [rdi], al
    mov [rdi], dword 5
    add [counter], 1        ;~
    inc [rdi]               ;~
    inc qword [rdi]
    cmp [rsi], 10           ;~
    cmp byte [rsi], 10
    shl [rdi], 1            ;~
    div [rbx]               ;~
    div qword [rbx]
    push [rdi]
    ret

section .data
counter dq 0
