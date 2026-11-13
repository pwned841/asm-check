section .data
first db "hello"
first_len equ $ - first
second db "abc"
third db "defg"
second_len equ $ - second       ;~
multi db "line one", 10
      db "line two", 10
multi_len equ $ - multi
SIZE equ 4
