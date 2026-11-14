section .text
f:
    mov byte [buffer], 'H'  ;~
    mov byte [buffer + 1], 'i'
    mov byte [buffer + 2], '!'
    mov byte [buffer + 3], 10
    mov byte [rdi], 1       ;~
    mov byte [rdi + 1], 2
    mov byte [rsi], 0
    mov byte [rsi + 2], 0
    mov byte [rdx + 1], 5
    mov byte [rdx], 4
    mov byte [rcx + r8], 'a' ;~
    mov byte [rcx + r8 + 1], 'b'
    mov byte [rcx + r8 + 2], 'c'
    mov word [r9], 1
    ret

section .bss
buffer resb 16
