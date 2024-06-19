package org.vi_server.wgserver;


import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.os.AsyncTask;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.widget.Button;
import android.widget.EditText;
import android.widget.TextView;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.SocketException;
import java.net.UnknownHostException;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public class MainActivity extends Activity {

    private ExecutorService executorService;
    private Handler mainThreadHandler;
    private ServerSocket serverSocket;
    private Socket clientSocket;
    private DatagramSocket udpSocket;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);

        executorService = Executors.newFixedThreadPool(5);
        mainThreadHandler = new Handler(Looper.getMainLooper());

        Context ctx = this;

        {
            Button b = findViewById(R.id.bStart);
            b.setOnClickListener(view -> {
                Intent intent = new Intent(ctx, Serv.class);

                EditText t = findViewById(R.id.tConfig);
                String config = t.getText().toString();

                long instance = Native.create();
                String ret = Native.setConfig(instance, config);

                TextView s = findViewById(R.id.tStatus);
                if (ret != null && !ret.isEmpty()) {
                    s.setText(ret);
                } else {
                    intent.putExtra("instance", instance);

                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                        ctx.startForegroundService(intent);
                    } else {
                        ctx.startService(intent);
                    }
                    s.setText("started");
                }

            });
        }
        {
            Button b = findViewById(R.id.bStop);
            b.setOnClickListener(view -> {
                TextView s = findViewById(R.id.tStatus);
                Intent intent = new Intent(ctx, Serv.class);
                ctx.stopService(intent);
                s.setText("stopped");
            });
        }

        {
            Button b = findViewById(R.id.bCustomAction);
            b.setOnClickListener(view -> {
                sendUdpPacket();
//                listenTcp();
//                listenUdp();
            });
        }

        {
            Button b = findViewById(R.id.bSampleConfig);
            b.setOnClickListener(view -> {
                EditText t = findViewById(R.id.tConfig);
                t.setText(Native.getSampleConfig());
            });
        }
    }

    private void safeSetText(final String msg) {
        mainThreadHandler.post(() -> {
                    EditText t = findViewById(R.id.tConfig);
                    t.setText(msg);
                }
        );
    }

    private void listenUdp() {
        executorService.execute(() -> {
            String msg = "";
            if (udpSocket != null) {
                udpSocket.close();
                udpSocket = null;
                msg += "udpSocket was running, now closed\n";
            }
            if (!msg.isEmpty()) {
                safeSetText(msg);
                return;
            }

            try {
                udpSocket = new DatagramSocket(4652, InetAddress.getByName("192.168.12.15"));
                msg += "created socket on port 4652\n";
                safeSetText(msg);

                byte[] buffer = new byte[1024];
                DatagramPacket packet = new DatagramPacket(buffer, buffer.length);
                udpSocket.receive(packet);
                String message = new String(packet.getData(), 0, packet.getLength());
                msg += "Received from " + packet.getAddress().getHostAddress() + ": " + message + "\n";
                safeSetText(msg);

                DatagramPacket outPacket = new DatagramPacket(message.getBytes(), message.length(), packet.getAddress(), packet.getPort());
                udpSocket.send(outPacket);

                msg += "Packet sent successfully!";
            } catch (Exception e) {
                msg += "error: " + e.getMessage() + "\n";
            } finally {
                if (udpSocket != null && !udpSocket.isClosed()) {
                    udpSocket.close();
                }
                safeSetText(msg);
            }
        });
    }

    private void listenTcp() {
        executorService.execute(() -> {
            String msg = "";
            if (clientSocket != null) {
                try {
                    clientSocket.close();
                } catch (IOException e) {
                    e.printStackTrace();
                }
                clientSocket = null;
                msg += "clientSocket was running, now closed\n";
            }
            if (serverSocket != null) {
                try {
                    serverSocket.close();
                } catch (IOException e) {
                    e.printStackTrace();
                }
                serverSocket = null;
                msg += "serverSocket was running, now closed\n";
            }
            if (!msg.isEmpty()) {
                safeSetText(msg);
                return;
            }

            try {
                serverSocket = new ServerSocket(4652, 10, InetAddress.getByName("192.168.12.15"));
                msg += "created socket on port 4652\n";
                safeSetText(msg);

                clientSocket = serverSocket.accept();
                msg += "accepting connection from " + clientSocket.getRemoteSocketAddress() + "\n";
                safeSetText(msg);

                InputStream input = null;
                try {
                    input = clientSocket.getInputStream();
                    byte[] buffer = new byte[1024];
                    int bufferOffset = 0;
                    long start = System.currentTimeMillis();
                    while (System.currentTimeMillis() - start < 5000 && bufferOffset < buffer.length) {
                        int readLength = java.lang.Math.min(input.available(), buffer.length - bufferOffset);
                        int readResult = input.read(buffer, bufferOffset, readLength);
                        if (readResult == -1) break;
                        bufferOffset += readResult;
                    }
                    msg += new String(buffer, 0, bufferOffset, StandardCharsets.UTF_8) + "\n";
                    safeSetText(msg);
                } catch (IOException e) {
                    msg += "error: " + e.getMessage() + "\n";
                    safeSetText(msg);
                } finally {
                    try {
                        if (input != null) {
                            input.close();
                        }
                        clientSocket.close();
                    } catch (IOException e) {
                        msg += "error: " + e.getMessage() + "\n";
                        safeSetText(msg);
                    }
                }

                clientSocket.close();
            } catch (IOException e) {
                safeSetText(e.getMessage());
            }
        });
    }

    private void sendUdpPacket() {
        executorService.execute(() -> {
            String result;
            try {
                String message = "Hello, UDP!";
                InetAddress address = InetAddress.getByName("192.168.12.1");
                int port = 4500;

                DatagramSocket socket = new DatagramSocket();
                DatagramPacket packet = new DatagramPacket(message.getBytes(), message.length(), address, port);
                socket.send(packet);

                socket.close();
                result = "Packet sent successfully!";
            } catch (Exception e) {
                e.printStackTrace();
                result = "Failed to send packet: " + e.getMessage();
            }

            safeSetText(result);
        });
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();

        if (serverSocket != null) {
            try {
                serverSocket.close();
            } catch (IOException e) {
                e.printStackTrace();
            }
        }

        if (udpSocket != null) {
            udpSocket.close();
        }

        executorService.shutdown();
    }
}
