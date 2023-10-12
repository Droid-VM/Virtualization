# Service VM

The Service VM is a lightweight bare-metal virtual machine specifically designed to run various services for other virtual machines. It fulfills the following requirements:

- The secret within the instance image remains constant across updates of the virtual machine.
- Only one instance of the Service VM is allowed to run at any given time.

## RKP VM (Remote Key Provisioning Virtual Machine)

The RKP VM is a key component of the Service VM. It is a virtual machine that undergoes validation by the RKP Server and serves as a trusted platform for verifying the integrity of other virtual machines.

### RKP VM attestation

The RKP VM is recognized and attested by the RKP server, which acts as a trusted entity responsible for verifying the authenticity and integrity of the RKP VM. This attestation process ensures that the RKP VM has not been tampered with or compromised.

### Client VM attestation

Once the RKP VM is successfully attested, it assumes the role of a trusted platform to attest client VMs. It leverages its trusted status to validate the integrity of the DICE chain associated with each client VM. This validation process ensures that the client VMs are running in a secure and trusted environment.