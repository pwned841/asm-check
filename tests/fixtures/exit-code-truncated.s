section .text
quit:
    mov edi, 256
    mov eax, 60
    syscall                 ;~
    mov rdi, -1
    mov eax, 231
    syscall                 ;~
    mov edi, 255
    mov eax, 60
    syscall
    xor edi, edi
    mov eax, 60
    syscall
    mov edi, 300
    mov eax, 1
    syscall
