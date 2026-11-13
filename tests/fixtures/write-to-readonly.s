section .text
f:
    mov byte [constant], 1      ;~
    inc qword [counter]
    mov al, [constant]
    mov [table + rax*8], rbx    ;~
    cmp byte [constant], 0
    mov byte [inline], 0        ;~
    ret
inline db 0

section .rodata
constant db 0
table dq 1, 2

section .data
counter dq 0
