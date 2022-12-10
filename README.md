# **WastleDB**

## **What is WastleDB?**:
WastleDB is the SQL database writed fully in **Rust** programming language. It offers support for **ANSI SQL** dialect

## **Status**:
**🛠️ Under Development 🛠️**

## **Tests**:
When you're interested let try to perform tests using **Cargo** project manager and it test command ``cargo test [test-name]`` after that you should have more knowledge how **WastleDB Connection Protocol** has been built and how other stuff has been solved. I appreciate your involvement

## **WastleDB protocol**:
WastleDB implemnting own application layer protocol (1 Layer of TCP/IP protocols model otherwise 7-5 layer from ISO/OSI model) under name **WastleDB Communication Protocol**.
This Protocol is using to perform communication between Database Server and client in order to send and recive data between communication peers.
</br>
To transport data it using TCP protocol from second layer of TCP/IP model
</br>
**WastleDB Communication Protocol** offers full support for communication encryption using for that Hybrid Encryption like TLS. To encrypt fundamentally data is using Symmetric Cipher Block encryption (AES-256 with GCM mode) but to secure AES key delivery is using PKI RSA-OAEP+ algorithm (from rust **rsa crate** (also created fully in rust and with security audit)).
The bigest difference in encryption between that what is implemented into **WastleDB Communication Protocol** and into TLS is that the RSA Public key must be knowed to client to perform encrypted connection

## **SQL support:**:
WastleDB uses **ANSI SQL dialect** so there are some bunch of differences between command ranges regard to other SQL dialects i.e: PostgreSQL, MySQL etc...
My database offer support for all the most used SQL commands but I'm in op to cover all SQL commands surface

## **Actual Version**:
<table>
    <tr>
        <th>Version</th>
        <th>Status</th>
    </tr>
    <tr>
        <td>0.5</td>
        <td>Alpha</td>
    </tr>
</table>

## **Client APIs**:

#### **Legend**:
❌ - **not supported | N/A**
<br>
✅ - **supported**

<table>
    <tr>
        <th>
            Language
        </th>
        <th>
            Status
        </th>
        <th>
            Version
        </th>
    </tr>
    <tr>
        <td>Rust</td>
        <td>❌</td>
        <td>❌</td>
    </tr>
    <tr>
        <td>Node.js</td>
        <td>❌</td>
        <td>❌</td>
    </tr>
    <tr>
        <td>Python</td>
        <td>❌</td>
        <td>❌</td>
    </tr>
    <tr>
        <td>Ada</td>
        <td>❌</td>
        <td>❌</td>
    </tr>
    <tr>
        <td>Ruby</td>
        <td>❌</td>
        <td>❌</td>
    </tr>
    <tr>
        <td>C / C++</td>
        <td>❌</td>
        <td>❌</td>
    </tr>
</table>
