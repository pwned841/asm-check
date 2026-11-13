extern puts, printf
section .text
main:
    push rbp
    mov rdi, greeting
    call puts               ;~
    lea rdi, [rel format]
    xor eax, eax
    call printf
    mov eax, 2
    mov rdi, path
    xor esi, esi
    syscall                 ;~
    mov eax, 2
    mov rdi, good_path
    syscall
    mov eax, 1
    mov rsi, greeting
    mov edx, 5
    syscall
    pop rbp
    xor eax, eax
    ret

section .data
greeting db "hello"
format db "%d", 10, 0
path db "/etc/passwd"
good_path db "/etc/", "hosts"
          db 0
