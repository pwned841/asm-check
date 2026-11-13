section .text
f:
    mov rax, [flag]             ;~
    mov al, [flag]
    movzx eax, byte [flag]
    mov [flag], rax             ;~
    mov qword [word_value], 1   ;~
    mov ax, [word_value]
    mov rax, [quad]
    mov rax, [message]          ;~
    mov eax, [message]
    cmp dword [buffer], '1337'
    mov rax, [buffer]
    mov rax, [flag + 1]
    lea rax, [flag]
    mov rax, [long_message]
    ret

section .data
flag db 0
word_value dw 0
quad dq 0
message db "hello"
long_message db "first "
             db "second"

section .bss
buffer resb 64
