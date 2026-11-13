; Calls printf in a loop, keeping the counter in a callee-saved register.
global main
extern printf

section .text
main:
    push rbx
    mov ebx, 3
.loop:
    lea rdi, [rel format]
    mov esi, ebx
    xor eax, eax
    call printf
    dec ebx
    jnz .loop
    xor eax, eax
    pop rbx
    ret

section .rodata
format db "%d", 10, 0
