section .text
print:
    mov eax, 1
    mov edi, 1
    mov rsi, [message]      ;~
    mov edx, 5
    syscall
    mov eax, 1
    mov rsi, message
    syscall
    mov eax, 1
    mov rsi, [pointer]
    syscall
    mov eax, 0
    mov rsi, [rel buffer]   ;~
    syscall
    mov eax, 3
    mov rdi, [message]
    syscall
    ret

section .data
message db "hello"
pointer dq message

section .bss
buffer resb 16
